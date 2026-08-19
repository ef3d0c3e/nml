//! The build pipeline shared by the CLI and the language server.

mod link;

pub use link::LinkError;
pub use link::Target;
pub use link::resolve;

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_diagnostics::DiagnosticSeverity;
use nml_document::Dependency;
use nml_document::Document;
use nml_document::ElementId;
use nml_document::ElementKind;
use nml_document::Export;
use nml_document::RenderKind;
use nml_document::Slot;
use nml_document::SlotId;
use nml_document::SymbolKind;
use nml_document::Use;
use nml_html::PageOptions;
use nml_html::Resolved;
use nml_lang::Compiled;
use nml_lang::Language;
use nml_lang::ResolveError;
use nml_lang::Resolver;
use nml_lang::output_path;
use nml_lang::resolve_file;
use nml_lang::resolve_path;
use nml_lua::DeferredRuntime;
use nml_lua::PageInfo;
use nml_render::Job;
use nml_render::RenderError;
use nml_render::scope_ids;
use nml_source::Source;
use nml_source::Token;
use nml_source::Url;
use nml_workspace::CONFIG_FILE;
use nml_workspace::Change;
use nml_workspace::Config;
use nml_workspace::Index;
use nml_workspace::IndexError;
use nml_workspace::Role;
use nml_workspace::ScanError;
use nml_workspace::ScannedFile;
use nml_workspace::StoredSlot;
use nml_workspace::StoredTemplate;
use nml_workspace::StoredUnit;
use nml_workspace::StoredUse;
use rayon::prelude::*;

use crate::link::Linker;

/// The result of a build.
#[derive(Debug, Default)]
pub struct BuildReport {
	/// Units parsed again, by path.
	pub parsed: Vec<String>,
	/// Pages written: parsed again, or relinked because a symbol they use changed.
	pub written: Vec<String>,
	/// Units that did not parse. Their last valid state stays in the index, and their
	/// page is not rewritten.
	pub failed: Vec<String>,
	/// Pages whose output was deleted: removed, or no longer pages.
	pub deleted: Vec<String>,
	pub diagnostics: Vec<Diagnostic>,
}

impl BuildReport {
	pub fn has_errors(&self) -> bool {
		has_errors(&self.diagnostics)
	}
}

fn has_errors(diagnostics: &[Diagnostic]) -> bool {
	diagnostics
		.iter()
		.any(|d| d.severity == DiagnosticSeverity::Error)
}

/// A build failure that is not about a source: a missing configuration, an I/O or database
/// error.
#[derive(Debug)]
pub enum BuildError {
	NoConfig(PathBuf),
	Io(PathBuf, io::Error),
	Index(IndexError),
}

impl std::fmt::Display for BuildError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			BuildError::NoConfig(root) => {
				write!(f, "no {CONFIG_FILE} in {}", root.display())
			}
			BuildError::Io(path, error) => write!(f, "{}: {error}", path.display()),
			BuildError::Index(error) => error.fmt(f),
		}
	}
}

impl std::error::Error for BuildError {}

impl From<IndexError> for BuildError {
	fn from(error: IndexError) -> Self {
		Self::Index(error)
	}
}

impl From<ScanError> for BuildError {
	fn from(error: ScanError) -> Self {
		match error {
			ScanError::Io(path, error) => Self::Io(path, error),
			ScanError::Index(error) => Self::Index(error),
		}
	}
}

fn source(path: &Path, content: String) -> Arc<Source> {
	let url = Url::from_file_path(path).expect("workspace paths are absolute");
	Arc::new(Source::new(url, content, 0))
}

fn read(path: &Path) -> Result<Arc<Source>, BuildError> {
	let content = fs::read_to_string(path).map_err(|e| BuildError::Io(path.into(), e))?;
	Ok(source(path, content))
}

/// Builds the workspace at `root` incrementally: changed files are parsed again, and pages
/// whose references may resolve differently are relinked from their stored templates.
pub fn build(root: &Path) -> Result<BuildReport, BuildError> {
	build_with(root, BuildOptions::default())
}

/// How to build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildOptions {
	/// Write the pages and delete the outputs of removed ones. The language server builds
	/// without writing: it only needs the index and the diagnostics.
	pub write: bool,
}

impl Default for BuildOptions {
	fn default() -> Self {
		Self { write: true }
	}
}

/// Builds the workspace at `root` incrementally, with options.
pub fn build_with(root: &Path, options: BuildOptions) -> Result<BuildReport, BuildError> {
	let root = fs::canonicalize(root).map_err(|e| BuildError::Io(root.into(), e))?;
	let config_path = root.join(CONFIG_FILE);
	if !config_path.exists() {
		return Err(BuildError::NoConfig(root));
	}
	let config_source = read(&config_path)?;
	let config = match Config::parse(config_source.clone()) {
		Ok(config) => config,
		Err(diagnostics) => {
			return Ok(BuildReport {
				diagnostics,
				..BuildReport::default()
			});
		}
	};
	let config_hash = hex(&nml_workspace::hash(config_source.content().as_bytes()));
	let index = Index::open(&root.join(&config.database))?;
	let resolver: Arc<dyn Resolver> =
		Arc::new(WorkspaceResolver::new(root.clone(), config.modules.clone()));
	let mut builder = Builder {
		write: options.write,
		output: root.join(&config.output),
		root,
		config,
		resolver,
		pages: None,
		pages_changed: false,
		index,
		language: language(),
		sources: HashMap::new(),
		affected: BTreeSet::new(),
		to_link: BTreeSet::new(),
		report: BuildReport::default(),
	};
	builder.run(&config_hash)?;
	Ok(builder.report)
}

struct Builder {
	/// Whether outputs are written; see [`BuildOptions::write`].
	write: bool,
	root: PathBuf,
	config: Config,
	resolver: Arc<dyn Resolver>,
	/// The pages of the workspace, for deferred functions, once every unit is committed.
	pages: Option<Arc<Vec<PageInfo>>>,
	/// Whether a page was added, removed, or changed its key or title: pages that list the
	/// workspace's pages are relinked.
	pages_changed: bool,
	index: Index,
	output: PathBuf,
	language: Language,
	/// The sources read during this build, by path.
	sources: HashMap<String, Arc<Source>>,
	/// Symbols whose resolution may have changed, as (unit key, name). An empty name stands
	/// for the unit's page.
	affected: BTreeSet<(String, String)>,
	/// Pages to write, by path.
	to_link: BTreeSet<String>,
	report: BuildReport,
}

impl Builder {
	fn run(&mut self, config_hash: &str) -> Result<(), BuildError> {
		let scan = nml_workspace::scan(&self.root, &self.config, &self.index)?;
		for path in &scan.removed {
			self.remove(path)?;
		}

		// Invalidate: changed files, everything if the configuration changed, and the
		// dependents of both, transitively.
		let config_changed = self.index.meta(CONFIG_HASH)?.as_deref() != Some(config_hash);
		let mut invalid: BTreeSet<String> = scan
			.files
			.iter()
			.filter(|file| config_changed || file.change != Change::Unchanged)
			.map(|file| file.path.clone())
			.collect();
		let existing: BTreeSet<&str> = scan.files.iter().map(|f| f.path.as_str()).collect();
		// Units whose scripts read a file that changed or disappeared.
		let mut hashes: HashMap<String, Option<String>> = HashMap::new();
		for (unit, path, hash) in self.index.file_dependencies()? {
			let current = hashes.entry(path.clone()).or_insert_with(|| {
				fs::read(self.root.join(&path))
					.ok()
					.map(|bytes| hex(&nml_workspace::hash(&bytes)))
			});
			if current.as_deref() != Some(hash.as_str()) && existing.contains(unit.as_str()) {
				invalid.insert(unit);
			}
		}
		let seeds = invalid.iter().chain(&scan.removed).cloned().collect();
		invalidate(&self.index, &self.config, seeds, &existing, &mut invalid)?;

		let mut to_parse = Vec::new();
		for file in &scan.files {
			if invalid.contains(&file.path) {
				let source = match &file.content {
					Some(content) => source(&file.absolute, content.clone()),
					None => read(&file.absolute)?,
				};
				to_parse.push((file, source));
				continue;
			}
			let stored = self.index.unit(&file.path)?.unwrap();
			if file.refresh(&stored.state) {
				self.index.update_state(&file.path, &file.state)?;
			}
		}

		// Units parse in parallel; they are committed in path order.
		let language = &self.language;
		let resolver = &self.resolver;
		let compiled: Vec<_> = to_parse
			.par_iter()
			.map(|(file, source)| {
				nml_lang::compile(language, source.clone(), &file.path, resolver.clone())
			})
			.collect();
		for ((file, source), compiled) in to_parse.into_iter().zip(compiled) {
			self.commit(file, source, compiled)?;
		}
		self.index.set_meta(CONFIG_HASH, config_hash)?;

		// Relink the pages whose last link had errors, those that use an affected symbol, and
		// those with deferred slots if the pages changed.
		if self.pages_changed {
			for path in self.index.units_with_deferred_slots()? {
				self.to_link.insert(path);
			}
		}
		for path in self.index.units_with_link_errors()? {
			self.to_link.insert(path);
		}
		for (unit, name) in std::mem::take(&mut self.affected) {
			for id in self.index.users(&unit, &name)? {
				let user = self.index.unit_by_id(id)?.unwrap();
				if user.role == Role::Page {
					self.to_link.insert(user.path);
				}
			}
		}
		for path in std::mem::take(&mut self.to_link) {
			self.link(&path)?;
		}
		// Renders no unit uses any more, since a unit changed or went away.
		self.index.prune_renders()?;
		self.write_code_stylesheet()
	}

	/// Renders the document's math, graphs and the like with external programs: their SVG
	/// by element, and the cache keys they use. Outputs are cached in the index by what they
	/// render; the others render in parallel. Failures are added to `diagnostics`.
	fn render(
		&self,
		document: &Document,
		diagnostics: &mut Vec<Diagnostic>,
	) -> Result<(HashMap<ElementId, String>, Vec<String>), BuildError> {
		let renderers = &self.config.render;
		let mut rendered = HashMap::new();
		let mut keys = Vec::new();
		let mut missing = Vec::new();
		for (id, element) in document.descendants(document.root()) {
			let ElementKind::Rendered {
				kind,
				source,
				block,
			} = &element.kind
			else {
				continue;
			};
			let job = match kind {
				RenderKind::Math => Job::Math {
					source: source.clone(),
					display: *block,
				},
				RenderKind::Tex { environment } => Job::Tex {
					source: source.clone(),
					environment: environment.clone(),
				},
				RenderKind::Graph { layout } => Job::Graph {
					source: source.clone(),
					layout: layout.clone(),
				},
				_ => Job::Typst {
					source: source.clone(),
				},
			};
			let environment = match renderers.environment_key(&job) {
				Ok(environment) => environment,
				Err(error) => {
					diagnostics.push(render_error(&error, &element.token));
					continue;
				}
			};
			let key = hex(&nml_workspace::hash(
				format!("{}\0{environment}\0{source}", kind.name()).as_bytes(),
			));
			keys.push(key.clone());
			match self.index.render(&key)? {
				Some(svg) => {
					rendered.insert(id, svg);
				}
				None => missing.push((id, key, job, element.token.clone())),
			}
		}
		let results: Vec<_> = missing
			.into_par_iter()
			.map(|(id, key, job, token)| {
				// Ids in an SVG are scoped by its key, so that several can share a page.
				let svg = renderers.render(&job).map(|svg| scope_ids(&svg, &key[..8]));
				(id, key, svg, token)
			})
			.collect();
		for (id, key, svg, token) in results {
			match svg {
				Ok(svg) => {
					self.index.store_render(&key, &svg)?;
					rendered.insert(id, svg);
				}
				Err(error) => diagnostics.push(render_error(&error, &token)),
			}
		}
		keys.sort();
		keys.dedup();
		Ok((rendered, keys))
	}

	/// Copies a file a page shows to the same path in the output directory, unless the copy
	/// is up to date: as large as the file, and not older.
	fn copy_asset(&self, path: &str) -> Result<(), BuildError> {
		if !self.write {
			return Ok(());
		}
		let from = self.root.join(path);
		let to = self.output.join(path);
		let metadata = fs::metadata(&from).map_err(|e| BuildError::Io(from.clone(), e))?;
		if let Ok(copy) = fs::metadata(&to)
			&& copy.len() == metadata.len()
			&& let (Ok(copied), Ok(modified)) = (copy.modified(), metadata.modified())
			&& copied >= modified
		{
			return Ok(());
		}
		if let Some(parent) = to.parent() {
			fs::create_dir_all(parent).map_err(|e| BuildError::Io(parent.into(), e))?;
		}
		fs::copy(&from, &to).map_err(|e| BuildError::Io(from, e))?;
		Ok(())
	}

	/// Writes the stylesheet of highlighted code, if it changed.
	fn write_code_stylesheet(&self) -> Result<(), BuildError> {
		if !self.write {
			return Ok(());
		}
		// The theme was checked with the configuration.
		let css = nml_render::stylesheet(&self.config.html.code_theme).unwrap_or_default();
		let path = self.output.join(nml_html::CODE_STYLESHEET);
		if fs::read_to_string(&path).is_ok_and(|current| current == css) {
			return Ok(());
		}
		fs::create_dir_all(&self.output).map_err(|e| BuildError::Io(self.output.clone(), e))?;
		fs::write(&path, css).map_err(|e| BuildError::Io(path, e))
	}

	fn remove(&mut self, path: &str) -> Result<(), BuildError> {
		let Some(unit) = self.index.unit(path)? else {
			return Ok(());
		};
		for change in self.index.remove(path)? {
			self.affected.insert((unit.name.clone(), change.name));
		}
		self.affected.insert((unit.name, String::new()));
		if unit.role == Role::Page {
			self.pages_changed = true;
			self.delete_output(path)?;
		}
		Ok(())
	}

	/// Deletes the output of a page that is gone or no longer a page, and the directories
	/// it leaves empty. Only pages the index knows are deleted: other files in the output
	/// directory are left alone.
	fn delete_output(&mut self, path: &str) -> Result<(), BuildError> {
		if !self.write {
			return Ok(());
		}
		let target = self.output.join(output_path(path));
		match fs::remove_file(&target) {
			Ok(()) => {}
			Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
			Err(e) => return Err(BuildError::Io(target, e)),
		}
		self.report.deleted.push(path.to_string());
		let mut dir = target.parent();
		while let Some(current) = dir.filter(|d| *d != self.output && d.starts_with(&self.output)) {
			// Fails when the directory is not empty, which ends the walk.
			if fs::remove_dir(current).is_err() {
				break;
			}
			dir = current.parent();
		}
		Ok(())
	}

	/// Commits a parsed unit, unless it has errors.
	fn commit(
		&mut self,
		file: &ScannedFile,
		source: Arc<Source>,
		compiled: Compiled,
	) -> Result<(), BuildError> {
		self.sources.insert(file.path.clone(), source);
		self.report.parsed.push(file.path.clone());
		let mut compiled = compiled;
		let (rendered, render_keys) =
			if file.role == Role::Page && !has_errors(&compiled.diagnostics) {
				self.render(&compiled.unit.document, &mut compiled.diagnostics)?
			} else {
				(HashMap::new(), Vec::new())
			};
		let failed = has_errors(&compiled.diagnostics);
		self.report.diagnostics.extend(compiled.diagnostics);
		if failed {
			self.report.failed.push(file.path.clone());
			return Ok(());
		}

		let unit = compiled.unit;
		if file.role == Role::Page {
			for asset in &unit.assets {
				self.copy_asset(asset)?;
			}
		}
		let document = &unit.document;
		let basename = file.path.rsplit('/').next().unwrap();
		let basename = basename.strip_suffix(".nml").unwrap_or(basename);
		let name = unit.reference_key.as_deref().unwrap_or(basename);
		let title = title(document).unwrap_or_else(|| basename.to_string());
		let template = (file.role == Role::Page).then(|| {
			let mut stylesheets = self.config.html.stylesheets.clone();
			let has_code = document
				.descendants(document.root())
				.any(|(_, element)| matches!(element.kind, ElementKind::CodeBlock { .. }));
			if has_code {
				stylesheets.push(nml_lang::relative_output_url(
					&file.path,
					nml_html::CODE_STYLESHEET,
				));
			}
			let options = PageOptions {
				language: self.config.html.language.clone(),
				title: title.clone(),
				stylesheets,
				icon: self.config.html.icon.clone(),
				rendered,
			};
			store_template(document, &nml_html::compile(document, &options))
		});
		let uses: Vec<_> = unit.uses.iter().map(store_use).collect();

		let old = self.index.unit(&file.path)?;
		let old_exports = match &old {
			Some(old) => self.index.exports(old.id)?,
			None => Vec::new(),
		};
		// Deferred functions see the variables pages export.
		if file.role == Role::Page && variables(&old_exports) != variables(&unit.exports) {
			self.pages_changed = true;
		}
		let (id, changes) = self.index.commit(&StoredUnit {
			path: &file.path,
			name,
			title: &title,
			role: file.role,
			state: file.state.clone(),
			exports: &unit.exports,
			uses: &uses,
			dependencies: &unit.dependencies,
			template: template.as_ref(),
		})?;
		self.index.set_unit_renders(id, &render_keys)?;
		for change in changes {
			self.affected.insert((name.to_string(), change.name));
		}
		// What dependents see of the unit itself: its key, title and role. If they changed,
		// every reference to the unit may resolve differently.
		let identity_changed = old
			.as_ref()
			.is_none_or(|old| old.name != name || old.title != title || old.role != file.role);
		if identity_changed {
			self.pages_changed = true;
			self.affected.insert((name.to_string(), String::new()));
			for export in &unit.exports {
				self.affected
					.insert((name.to_string(), export.name.clone()));
			}
		}
		let was_page = old.as_ref().is_some_and(|old| old.role == Role::Page);
		let old_exports = if old.as_ref().is_some_and(|old| old.name != name) {
			old_exports
		} else {
			Vec::new()
		};
		if let Some(old) = old.filter(|old| old.name != name) {
			self.affected.insert((old.name.clone(), String::new()));
			for export in old_exports {
				self.affected.insert((old.name.clone(), export.name));
			}
		}
		if file.role == Role::Page {
			self.to_link.insert(file.path.clone());
		} else if was_page {
			self.delete_output(&file.path)?;
		}
		Ok(())
	}

	/// Writes a page from its stored template, filling its slots.
	fn link(&mut self, path: &str) -> Result<(), BuildError> {
		let unit = self.index.unit(path)?.unwrap();
		let template = self.index.template(unit.id)?.unwrap();
		let source = match self.sources.get(path) {
			Some(source) => source.clone(),
			None => read(&self.root.join(path))?,
		};
		let target = self.output.join(output_path(path));
		let io_error = |e| BuildError::Io(target.clone(), e);
		let mut file: Box<dyn io::Write> = if self.write {
			if let Some(parent) = target.parent() {
				fs::create_dir_all(parent).map_err(|e| BuildError::Io(parent.into(), e))?;
			}
			Box::new(io::BufWriter::new(
				fs::File::create(&target).map_err(io_error)?,
			))
		} else {
			Box::new(io::sink())
		};

		let pages = match &self.pages {
			Some(pages) => pages.clone(),
			None => {
				let mut pages = Vec::new();
				for unit in self.index.units()? {
					if unit.role == Role::Page {
						let variables = variables(&self.index.exports(unit.id)?);
						pages.push(PageInfo {
							path: unit.path,
							name: unit.name,
							title: unit.title,
							variables,
						});
					}
				}
				self.pages.insert(Arc::new(pages)).clone()
			}
		};
		let mut deferred = None;
		let linker = Linker {
			index: &self.index,
			unit: &unit,
		};
		let positions: Vec<_> = template
			.slots
			.iter()
			.map(|slot| (slot.position, slot.context))
			.collect();
		let mut index_error = None;
		let diagnostics = nml_html::write(
			&template.content,
			&positions,
			|index| {
				let stored = &template.slots[index];
				let link = match &stored.slot {
					Slot::Link(link) => link,
					Slot::Deferred(slot) => {
						let runtime = deferred.get_or_insert_with(|| {
							DeferredRuntime::new(
								self.resolver.clone(),
								pages.clone(),
								path.to_string(),
							)
						});
						let token = Token::new(source.clone(), stored.range.clone());
						return match runtime.fill(slot, &token) {
							Ok(html) => Resolved::Value(html),
							Err(diagnostic) => Resolved::Missing(diagnostic),
						};
					}
					_ => return Resolved::Value(String::new()),
				};
				let target = link.target.to_string();
				let kind = match linker.value(link) {
					Ok(value) => return Resolved::Value(value),
					Err(LinkError::Unresolved) => DiagnosticKind::UnresolvedReference { target },
					Err(LinkError::Ambiguous(count)) => {
						DiagnosticKind::AmbiguousReference { target, count }
					}
					Err(LinkError::Index(error)) => {
						index_error = Some(error);
						return Resolved::Value(String::new());
					}
				};
				let token = Token::new(source.clone(), stored.range.clone());
				Resolved::Missing(Diagnostic::new(kind, DiagnosticMessage::new(token)))
			},
			&mut file,
		)
		.map_err(io_error)?;
		if let Some(error) = index_error {
			return Err(error.into());
		}
		// A reference has a slot for its URL and one for its title: report it once.
		let mut reported: Vec<Diagnostic> = Vec::new();
		for diagnostic in diagnostics {
			let range = diagnostic.primary().location.range();
			let duplicate = reported.iter().any(|other| {
				let other_range = other.primary().location.range();
				other.kind == diagnostic.kind
					&& other_range.start <= range.start
					&& range.end <= other_range.end
			});
			if !duplicate {
				reported.push(diagnostic);
			}
		}
		self.index.set_link_errors(path, has_errors(&reported))?;
		self.report.diagnostics.extend(reported);
		self.report.written.push(path.to_string());
		Ok(())
	}
}

/// The language of the workspace: the built-in rules and Lua.
pub fn language() -> Language {
	let mut language = Language::builtin();
	nml_lua::add(&mut language).expect("the Lua rules are valid");
	language
}

/// Finds included files, read files and imported modules in the workspace: in the overlay
/// first (unsaved buffers of an editor), then on disk.
#[derive(Debug, Clone)]
pub struct WorkspaceResolver {
	root: PathBuf,
	modules: Vec<nml_workspace::Module>,
	/// Contents by workspace path.
	overlay: HashMap<String, String>,
}

impl WorkspaceResolver {
	pub fn new(root: PathBuf, modules: Vec<nml_workspace::Module>) -> Self {
		Self {
			root,
			modules,
			overlay: HashMap::new(),
		}
	}

	/// Files whose content is taken from memory instead of the disk.
	pub fn with_overlay(mut self, overlay: HashMap<String, String>) -> Self {
		self.overlay = overlay;
		self
	}

	/// The content of a workspace file: from the overlay, or the disk.
	fn content(&self, path: &str) -> Option<String> {
		match self.overlay.get(path) {
			Some(content) => Some(content.clone()),
			None => fs::read_to_string(self.root.join(path)).ok(),
		}
	}
}

impl Resolver for WorkspaceResolver {
	fn asset(&self, from: &str, path: &str) -> Result<(String, String), ResolveError> {
		let not_found = |path: &str| ResolveError::NotFound { path: path.into() };
		let resolved = resolve_file(from, path).ok_or_else(|| not_found(path))?;
		let hash = match self.overlay.get(&resolved) {
			Some(content) => nml_workspace::hash(content.as_bytes()),
			None => {
				let bytes =
					fs::read(self.root.join(&resolved)).map_err(|_| not_found(&resolved))?;
				nml_workspace::hash(&bytes)
			}
		};
		Ok((resolved, hex(&hash)))
	}

	fn read(&self, from: &str, path: &str) -> Result<(String, String), ResolveError> {
		let not_found = |path: &str| ResolveError::NotFound { path: path.into() };
		let resolved = resolve_file(from, path).ok_or_else(|| not_found(path))?;
		let content = self
			.content(&resolved)
			.ok_or_else(|| not_found(&resolved))?;
		Ok((resolved, content))
	}

	fn include(&self, from: &str, path: &str) -> Result<(String, Arc<Source>), ResolveError> {
		let not_found = |path: &str| ResolveError::NotFound { path: path.into() };
		let resolved = resolve_path(from, path).ok_or_else(|| not_found(path))?;
		let content = self
			.content(&resolved)
			.ok_or_else(|| not_found(&resolved))?;
		Ok((
			resolved.clone(),
			source(&self.root.join(&resolved), content),
		))
	}

	fn module(&self, module: &str) -> Result<(String, Arc<Source>), ResolveError> {
		let (name, rest) = module.split_once('.').unwrap_or((module, ""));
		let Some(root) = self.modules.iter().find(|m| m.name == name) else {
			// The modules that come with NML, unless the workspace has its own `std`.
			if name == STD_MODULE {
				let path = format!("{STD_MODULE}/{rest}.nml");
				return match nml_lua::STD.iter().find(|(module, _)| *module == rest) {
					Some((_, content)) => {
						let url = Url::parse(&format!("nml:///{path}")).unwrap();
						Ok((path, Arc::new(Source::new(url, content.to_string(), 0))))
					}
					None => Err(ResolveError::NotFound { path }),
				};
			}
			return Err(ResolveError::UnknownModule { name: name.into() });
		};
		let root: Vec<_> = root
			.root
			.components()
			.filter_map(|c| match c {
				std::path::Component::Normal(part) => Some(part.to_string_lossy()),
				_ => None,
			})
			.collect();
		let path = format!("{}/{}.nml", root.join("/"), rest.replace('.', "/"));
		match self.content(&path) {
			Some(content) if !rest.is_empty() => {
				let absolute = self.root.join(&path);
				Ok((path, source(&absolute, content)))
			}
			_ => Err(ResolveError::NotFound { path }),
		}
	}
}

/// The diagnostic of a renderer that failed on the element at `token`.
fn render_error(error: &RenderError, token: &Token) -> Diagnostic {
	let kind = DiagnosticKind::RenderError {
		message: error.to_string(),
	};
	Diagnostic::new(kind, DiagnosticMessage::new(token.clone()))
}

/// The name of the modules that come with NML.
const STD_MODULE: &str = "std";

/// The exported variables of a unit, with their values as written.
fn variables(exports: &[Export]) -> Vec<(String, String)> {
	exports
		.iter()
		.filter(|export| export.kind == SymbolKind::Variable)
		.map(|export| {
			let value = export.property("value").unwrap_or_default();
			(export.name.clone(), value.to_string())
		})
		.collect()
}

/// Bytes in lowercase hexadecimal.
fn hex(bytes: &[u8]) -> String {
	bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The key of the configuration's hash in the index: a new configuration invalidates every
/// unit.
const CONFIG_HASH: &str = "config_hash";

/// Adds to `invalid` the units that depend, transitively, on the units at the `seeds`
/// paths: they include it, or import it as a module. Only existing files are added.
fn invalidate(
	index: &Index,
	config: &Config,
	seeds: Vec<String>,
	existing: &BTreeSet<&str>,
	invalid: &mut BTreeSet<String>,
) -> Result<(), BuildError> {
	let mut queue = seeds;
	while let Some(path) = queue.pop() {
		let mut dependencies = vec![Dependency::Include(path.clone())];
		if let Some(module) = config.module_path(&path) {
			dependencies.push(Dependency::Import(module));
		}
		for dependency in dependencies {
			for dependent in index.dependents(&dependency)? {
				if existing.contains(dependent.as_str()) && invalid.insert(dependent.clone()) {
					queue.push(dependent);
				}
			}
		}
	}
	Ok(())
}

/// The text of the document's first heading.
fn title(document: &Document) -> Option<String> {
	document
		.descendants(document.root())
		.find(|(_, element)| matches!(element.kind, ElementKind::Heading { .. }))
		.map(|(id, _)| document.text(id))
}

fn store_use(stored: &Use) -> StoredUse {
	StoredUse {
		target: stored.target.clone(),
		range: stored.token.range(),
	}
}

/// A compiled template as stored, with the source range of each slot.
fn store_template(document: &Document, template: &nml_html::Template) -> StoredTemplate {
	let mut ranges: HashMap<SlotId, std::ops::Range<usize>> = HashMap::new();
	for (_, element) in document.descendants(document.root()) {
		if let ElementKind::Slot(id) | ElementKind::Reference { href: id } = element.kind {
			ranges.insert(id, element.token.range());
		}
	}
	StoredTemplate {
		content: template.content.clone(),
		slots: template
			.slots
			.iter()
			.map(|slot| StoredSlot {
				position: slot.position,
				context: slot.context,
				range: ranges[&slot.slot].clone(),
				slot: document.slot(slot.slot).clone(),
			})
			.collect(),
	}
}

#[cfg(test)]
mod tests;
