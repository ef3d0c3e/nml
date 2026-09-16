//! The build pipeline shared by the CLI and the language server.

mod link;

pub use link::LinkError;
pub use link::Target;
pub use link::label;
pub use link::resolve;

use std::collections::BTreeMap;
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
use nml_render::recolor;
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
	let scan = nml_workspace::scan(&root, &config, &index)?;
	let mut builder = Builder {
		write: options.write,
		single_file: false,
		config_source,
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
	builder.run(&config_hash, scan)?;
	Ok(builder.report)
}

/// Builds one file without a workspace: default configuration, an index in memory, and
/// the page written to `output` (by default, next to the file with the `.html` extension),
/// with its assets in `assets/` beside it. Files it includes and shows are found from its
/// directory; it can import no module.
pub fn build_file(file: &Path, output: Option<&Path>) -> Result<BuildReport, BuildError> {
	let file = fs::canonicalize(file).map_err(|e| BuildError::Io(file.into(), e))?;
	let root = file.parent().unwrap().to_path_buf();
	let name = file.file_name().unwrap().to_string_lossy().to_string();
	let output = match output {
		Some(output) => {
			std::path::absolute(output).map_err(|e| BuildError::Io(output.into(), e))?
		}
		None => file.with_extension("html"),
	};
	let output_dir = output.parent().unwrap().to_path_buf();

	let config_source = Arc::new(Source::new(
		Url::parse("virtual:///nml.toml").unwrap(),
		String::new(),
		0,
	));
	let mut config =
		Config::parse(config_source.clone()).expect("the default configuration is valid");
	config.output = output_dir.clone();
	let content = fs::read_to_string(&file).map_err(|e| BuildError::Io(file.clone(), e))?;
	let scanned = ScannedFile {
		path: name.clone(),
		absolute: file.clone(),
		role: Role::Page,
		state: nml_workspace::FileState {
			mtime: 0,
			size: content.len() as u64,
			hash: nml_workspace::hash(content.as_bytes()),
		},
		content: Some(content),
		change: Change::Added,
	};
	let scan = nml_workspace::Scan {
		files: vec![scanned],
		removed: Vec::new(),
	};
	let resolver: Arc<dyn Resolver> = Arc::new(WorkspaceResolver::new(root.clone(), Vec::new()));
	let mut builder = Builder {
		write: true,
		single_file: true,
		config_source,
		output: output_dir,
		root,
		config,
		resolver,
		pages: None,
		pages_changed: false,
		index: Index::open_in_memory()?,
		language: language(),
		sources: HashMap::new(),
		affected: BTreeSet::new(),
		to_link: BTreeSet::new(),
		report: BuildReport::default(),
	};
	builder.run("", scan)?;
	// The page is written under its own name, then renamed. A page that `nml.output` puts
	// in another directory stays there, where its links to assets work.
	if let Some(unit) = builder.index.unit(&name)?
		&& builder.report.written.contains(&name)
	{
		let written = builder.output.join(&unit.output);
		if written != output && written.parent() == output.parent() {
			fs::rename(&written, &output).map_err(|e| BuildError::Io(written, e))?;
		}
	}
	Ok(builder.report)
}

struct Builder {
	/// Whether outputs are written; see [`BuildOptions::write`].
	write: bool,
	/// Whether one file is built without a workspace: the output directory may hold the
	/// files of other builds, which are left alone.
	single_file: bool,
	root: PathBuf,
	config_source: Arc<Source>,
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
	fn run(&mut self, config_hash: &str, scan: nml_workspace::Scan) -> Result<(), BuildError> {
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
		// Units whose scripts read an environment variable that changed, or the clock.
		for (unit, name, hash) in self.index.env_dependencies()? {
			let current = nml_lang::env_hash(self.resolver.env(&name).as_deref());
			if current != hash && existing.contains(unit.as_str()) {
				invalid.insert(unit);
			}
		}
		for unit in self.index.dependents(&Dependency::Clock)? {
			if existing.contains(unit.as_str()) {
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
		// Pages a build that does not write committed or relinked since they were written.
		if self.write {
			for path in self.index.unwritten_pages()? {
				self.to_link.insert(path);
			}
		}
		for (unit, name) in std::mem::take(&mut self.affected) {
			for id in self.index.users(&unit, &name)? {
				let user = self.index.unit_by_id(id)?.unwrap();
				if user.role == Role::Page {
					self.to_link.insert(user.path);
				}
			}
		}
		let statics = self.static_files()?;
		let conflicts = self.output_conflicts(&statics)?;
		for path in std::mem::take(&mut self.to_link) {
			if !conflicts.contains(&path) {
				self.link(&path)?;
			}
		}
		// Outputs a build that does not write left behind, unless a page is written there now.
		if self.write {
			for (output, path) in self.index.take_stale_outputs()? {
				if self.delete_file(&output)? {
					self.report.deleted.push(path);
				}
			}
		}
		// Renders no unit uses any more, since a unit changed or went away.
		self.index.prune_renders()?;
		self.sync_assets()?;
		self.sync_static(&statics)?;
		self.write_code_stylesheet()
	}

	/// The files of the static directory, by output path, with their path in the
	/// workspace. Those that would replace a file of the build are reported, and left out.
	fn static_files(&mut self) -> Result<BTreeMap<String, String>, BuildError> {
		let mut files = BTreeMap::new();
		let Some(dir) = &self.config.html.static_dir else {
			return Ok(files);
		};
		let location = || {
			Token::new(
				self.config_source.clone(),
				self.config.html.static_range.clone(),
			)
		};
		let absolute = self.root.join(dir);
		if !absolute.is_dir() {
			let kind = DiagnosticKind::MissingStaticDirectory {
				path: dir.to_string_lossy().into_owned(),
			};
			let diagnostic = Diagnostic::new(kind, DiagnosticMessage::new(location()));
			self.report.diagnostics.push(diagnostic);
			return Ok(files);
		}
		// The static directory may hold the workspace's own: it is not copied into itself.
		let skip = [self.output.clone(), self.root.join(".nml")];
		let mut paths = Vec::new();
		walk(&absolute, &skip, &mut paths)?;
		for path in paths {
			let slash = |path: &Path| {
				(path.components())
					.map(|c| c.as_os_str().to_string_lossy())
					.collect::<Vec<_>>()
					.join("/")
			};
			let output = slash(path.strip_prefix(&absolute).unwrap());
			let source = match path.strip_prefix(&self.root) {
				Ok(relative) => slash(relative),
				Err(_) => path.to_string_lossy().into_owned(),
			};
			let reserved = output == nml_html::CODE_STYLESHEET
				|| output.starts_with(&format!("{}/", nml_lang::ASSETS_DIR));
			if reserved {
				let kind = DiagnosticKind::ReservedStaticFile { path: output };
				let diagnostic = Diagnostic::new(kind, DiagnosticMessage::new(location()));
				self.report.diagnostics.push(diagnostic);
				continue;
			}
			files.insert(output, source);
		}
		Ok(files)
	}

	/// Copies the static files whose copy is missing or differs, and deletes the copies of
	/// those gone since the last build, unless a page is written there now.
	fn sync_static(&mut self, statics: &BTreeMap<String, String>) -> Result<(), BuildError> {
		if !self.write {
			return Ok(());
		}
		for (output, source) in statics {
			let from = self.root.join(source);
			let to = self.output.join(output);
			let content = fs::read(&from).map_err(|e| BuildError::Io(from.clone(), e))?;
			if fs::read(&to).is_ok_and(|current| current == content) {
				continue;
			}
			if let Some(parent) = to.parent() {
				fs::create_dir_all(parent).map_err(|e| BuildError::Io(parent.into(), e))?;
			}
			let mut partial = to.clone().into_os_string();
			partial.push(".partial");
			fs::write(&partial, &content).map_err(|e| BuildError::Io(from, e))?;
			fs::rename(&partial, &to).map_err(|e| BuildError::Io(to.clone(), e))?;
		}
		let pages: BTreeSet<String> = (self.index.units()?.into_iter())
			.filter(|unit| unit.role == Role::Page)
			.map(|unit| unit.output)
			.collect();
		let previous = self.index.meta(STATIC_FILES)?.unwrap_or_default();
		for output in previous.lines() {
			if !statics.contains_key(output) && !pages.contains(output) {
				self.delete_file(output)?;
			}
		}
		let current: Vec<&str> = statics.keys().map(String::as_str).collect();
		self.index.set_meta(STATIC_FILES, &current.join("\n"))?;
		Ok(())
	}

	/// Reports the pages written to the same output file, which are not written: an error
	/// in each, at its `nml.output` if it sets one. They are relinked once the conflict is
	/// gone.
	/// A page written where a static file is copied conflicts with it too.
	fn output_conflicts(
		&mut self,
		statics: &BTreeMap<String, String>,
	) -> Result<BTreeSet<String>, BuildError> {
		let mut by_output: HashMap<String, Vec<nml_workspace::UnitRecord>> = HashMap::new();
		for unit in self.index.units()? {
			if unit.role == Role::Page {
				by_output.entry(unit.output.clone()).or_default().push(unit);
			}
		}
		let mut conflicts = BTreeSet::new();
		let mut groups: Vec<_> = (by_output.into_iter())
			.filter_map(|(output, group)| {
				let file = statics.get(&output);
				(group.len() > 1 || file.is_some()).then_some((group, file))
			})
			.collect();
		groups.sort_by(|a, b| a.0[0].path.cmp(&b.0[0].path));
		for (group, file) in groups {
			for unit in &group {
				let other = match group.iter().find(|other| other.id != unit.id) {
					Some(other) => &other.path,
					None => file.unwrap(),
				};
				let source = match self.sources.get(&unit.path) {
					Some(source) => source.clone(),
					None => read(&self.root.join(&unit.path))?,
				};
				let kind = DiagnosticKind::OutputConflict {
					output: unit.output.clone(),
					other: other.clone(),
				};
				let token = Token::new(source, unit.output_range.clone().unwrap_or(0..0));
				self.report
					.diagnostics
					.push(Diagnostic::new(kind, DiagnosticMessage::new(token)));
				self.index.set_link_errors(&unit.path, true)?;
				conflicts.insert(unit.path.clone());
			}
		}
		Ok(conflicts)
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
				RenderKind::Graph { layout, .. } => Job::Graph {
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
					rendered.insert(id, recolor(&svg));
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
					// The cache keeps the program's output: how it is shown may change.
					self.index.store_render(&key, &svg)?;
					rendered.insert(id, recolor(&svg));
				}
				Err(error) => diagnostics.push(render_error(&error, &token)),
			}
		}
		keys.sort();
		keys.dedup();
		Ok((rendered, keys))
	}

	/// Copies the files pages show to the assets directory of the output, unless the copy
	/// exists (it is named by the file's content), and deletes the copies no page shows any
	/// more. The assets directory belongs to the build: other files in it are deleted too.
	fn sync_assets(&self) -> Result<(), BuildError> {
		if !self.write {
			return Ok(());
		}
		let assets = self.index.assets()?;
		for asset in &assets {
			let from = self.root.join(&asset.path);
			let to = self.output.join(&asset.output);
			// A file that went away fails the parse of the pages that show it.
			if to.exists() || !from.exists() {
				continue;
			}
			if let Some(parent) = to.parent() {
				fs::create_dir_all(parent).map_err(|e| BuildError::Io(parent.into(), e))?;
			}
			// Copied under another name first: an interrupted copy is not taken for the file.
			let mut partial = to.clone().into_os_string();
			partial.push(".partial");
			fs::copy(&from, &partial).map_err(|e| BuildError::Io(from, e))?;
			fs::rename(&partial, &to).map_err(|e| BuildError::Io(to.clone(), e))?;
		}
		if self.single_file {
			return Ok(());
		}
		let kept: BTreeSet<PathBuf> = assets
			.iter()
			.map(|asset| self.output.join(&asset.output))
			.collect();
		prune(&self.output.join(nml_lang::ASSETS_DIR), &kept)?;
		Ok(())
	}

	/// Writes the stylesheet of highlighted code, if it changed.
	fn write_code_stylesheet(&self) -> Result<(), BuildError> {
		if !self.write {
			return Ok(());
		}
		// One file is built into a directory of its own: only with code.
		if self.single_file {
			let mut uses_code = false;
			for unit in self.index.units()? {
				let template = self.index.template(unit.id)?;
				uses_code |=
					template.is_some_and(|t| t.content.contains(nml_html::CODE_STYLESHEET));
			}
			if !uses_code {
				return Ok(());
			}
		}
		// The themes were checked with the configuration.
		let html = &self.config.html;
		let css =
			nml_render::stylesheet(&html.code_theme, &html.code_theme_dark).unwrap_or_default();
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
			self.delete_output(path, &unit.output)?;
		}
		Ok(())
	}

	/// Deletes `output`, the output of the page at `path` that is gone, no longer a page, or
	/// written elsewhere now, and the directories it leaves empty. Only pages the index
	/// knows are deleted: other files in the output directory are left alone.
	/// A build that does not write records it, for the next build that writes.
	fn delete_output(&mut self, path: &str, output: &str) -> Result<(), BuildError> {
		if !self.write {
			self.index.add_stale_output(output, path)?;
		} else if self.delete_file(output)? {
			self.report.deleted.push(path.to_string());
		}
		Ok(())
	}

	/// Deletes `output` from the output directory, and the directories it leaves empty.
	/// Whether it existed.
	fn delete_file(&self, output: &str) -> Result<bool, BuildError> {
		let target = self.output.join(output);
		match fs::remove_file(&target) {
			Ok(()) => {}
			Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
			Err(e) => return Err(BuildError::Io(target, e)),
		}
		let mut dir = target.parent();
		while let Some(current) = dir.filter(|d| *d != self.output && d.starts_with(&self.output)) {
			// Fails when the directory is not empty, which ends the walk.
			if fs::remove_dir(current).is_err() {
				break;
			}
			dir = current.parent();
		}
		Ok(true)
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
		let output = match &unit.output {
			Some(token) => token.content().to_string(),
			None => output_path(&file.path),
		};
		// Where the file sets it; not if an included file does.
		let output_range = unit
			.output
			.as_ref()
			.filter(|token| self.sources[&file.path].location() == token.source().location())
			.map(Token::range);
		let document = &unit.document;
		let basename = file.path.rsplit('/').next().unwrap();
		let basename = basename.strip_suffix(".nml").unwrap_or(basename);
		let name = unit.reference_key.as_deref().unwrap_or(basename);
		let title = title(document).unwrap_or_else(|| basename.to_string());
		let template = (file.role == Role::Page).then(|| {
			let mut stylesheets: Vec<_> = (self.config.html.stylesheets.iter())
				.map(|url| site_url(&output, url))
				.collect();
			let has_code = document.descendants(document.root()).any(|(_, element)| {
				matches!(
					element.kind,
					ElementKind::CodeBlock { .. }
						| ElementKind::Code {
							language: Some(_),
							..
						}
				)
			});
			if has_code {
				stylesheets.push(nml_lang::relative_output_url(
					&output,
					nml_html::CODE_STYLESHEET,
				));
			}
			let options = PageOptions {
				language: self.config.html.language.clone(),
				title: title.clone(),
				stylesheets,
				icon: (self.config.html.icon.as_ref()).map(|url| site_url(&output, url)),
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
			output: &output,
			output_range,
			assets: if file.role == Role::Page {
				&unit.assets
			} else {
				&[]
			},
		})?;
		self.index.set_unit_renders(id, &render_keys)?;
		for change in changes {
			self.affected.insert((name.to_string(), change.name));
		}
		// What dependents see of the unit itself: its key, title and role. If they changed,
		// every reference to the unit may resolve differently.
		let identity_changed = old.as_ref().is_none_or(|old| {
			old.name != name || old.title != title || old.role != file.role || old.output != output
		});
		if identity_changed {
			self.pages_changed = true;
			self.affected.insert((name.to_string(), String::new()));
			for export in &unit.exports {
				self.affected
					.insert((name.to_string(), export.name.clone()));
			}
		}
		// The page's previous output, if it is not written there any more.
		let old_output = old
			.as_ref()
			.filter(|old| old.role == Role::Page)
			.filter(|old| file.role != Role::Page || old.output != output)
			.map(|old| old.output.clone());
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
		if let Some(old_output) = old_output {
			self.delete_output(&file.path, &old_output)?;
		}
		if file.role == Role::Page {
			self.to_link.insert(file.path.clone());
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
		let target = self.output.join(&unit.output);
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
							output: unit.output,
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
		self.index.set_written(path, self.write)?;
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
	/// When the resolver was created: the start of the build.
	time: i64,
}

impl WorkspaceResolver {
	pub fn new(root: PathBuf, modules: Vec<nml_workspace::Module>) -> Self {
		Self {
			root,
			modules,
			overlay: HashMap::new(),
			time: std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.map_or(0, |elapsed| elapsed.as_secs() as i64),
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

	fn env(&self, name: &str) -> Option<String> {
		std::env::var(name).ok()
	}

	fn time(&self) -> i64 {
		self.time
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
/// The URL of a configured stylesheet or icon in a page written at `output`. A relative URL
/// is relative to the output directory, so that it works from every page; an absolute one,
/// or one with a scheme, is kept.
fn site_url(output: &str, url: &str) -> String {
	let scheme = url.split_once(':').is_some_and(|(scheme, _)| {
		scheme.starts_with(|c: char| c.is_ascii_alphabetic())
			&& scheme
				.chars()
				.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
	});
	if scheme || url.starts_with('/') {
		url.to_string()
	} else {
		nml_lang::relative_output_url(output, url)
	}
}

/// The files of `dir`, except in the `skip` directories.
fn walk(dir: &Path, skip: &[PathBuf], files: &mut Vec<PathBuf>) -> Result<(), BuildError> {
	let entries = fs::read_dir(dir).map_err(|e| BuildError::Io(dir.into(), e))?;
	for entry in entries {
		let entry = entry.map_err(|e| BuildError::Io(dir.into(), e))?;
		let path = entry.path();
		let file_type = entry
			.file_type()
			.map_err(|e| BuildError::Io(path.clone(), e))?;
		if file_type.is_dir() && !skip.contains(&path) {
			walk(&path, skip, files)?;
		} else if file_type.is_file() {
			files.push(path);
		}
	}
	Ok(())
}

/// The output paths of the static files copied by the last build, one per line.
const STATIC_FILES: &str = "static_files";

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

/// Deletes the files under `dir` that are not in `kept`, and the directories left empty.
fn prune(dir: &Path, kept: &BTreeSet<PathBuf>) -> Result<(), BuildError> {
	let entries = match fs::read_dir(dir) {
		Ok(entries) => entries,
		Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
		Err(e) => return Err(BuildError::Io(dir.into(), e)),
	};
	for entry in entries {
		let entry = entry.map_err(|e| BuildError::Io(dir.into(), e))?;
		let path = entry.path();
		let file_type = entry
			.file_type()
			.map_err(|e| BuildError::Io(path.clone(), e))?;
		if file_type.is_dir() {
			prune(&path, kept)?;
			// Fails when the directory is not empty.
			let _ = fs::remove_dir(&path);
		} else if !kept.contains(&path) {
			fs::remove_file(&path).map_err(|e| BuildError::Io(path, e))?;
		}
	}
	Ok(())
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
