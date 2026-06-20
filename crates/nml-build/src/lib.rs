//! The build pipeline shared by the CLI and the language server.

mod link;

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
use nml_document::ElementKind;
use nml_document::Slot;
use nml_document::SlotId;
use nml_document::Use;
use nml_html::PageOptions;
use nml_html::Resolved;
use nml_lang::Compiled;
use nml_lang::Language;
use nml_lang::ResolveError;
use nml_lang::Resolver;
use nml_lang::resolve_path;
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

use crate::link::LinkError;
use crate::link::Linker;
use crate::link::output_path;

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
	let config_hash = nml_workspace::hash(config_source.content().as_bytes())
		.iter()
		.map(|byte| format!("{byte:02x}"))
		.collect::<String>();
	let index = Index::open(&root.join(&config.database))?;
	let mut builder = Builder {
		output: root.join(&config.output),
		root,
		config,
		index,
		language: Language::builtin(),
		sources: HashMap::new(),
		affected: BTreeSet::new(),
		to_link: BTreeSet::new(),
		report: BuildReport::default(),
	};
	builder.run(&config_hash)?;
	Ok(builder.report)
}

struct Builder {
	root: PathBuf,
	config: Config,
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
		let resolver = WorkspaceResolver {
			root: &self.root,
			config: &self.config,
		};
		let compiled: Vec<_> = to_parse
			.par_iter()
			.map(|(file, source)| {
				nml_lang::compile(language, source.clone(), &file.path, &resolver)
			})
			.collect();
		for ((file, source), compiled) in to_parse.into_iter().zip(compiled) {
			self.commit(file, source, compiled)?;
		}
		self.index.set_meta(CONFIG_HASH, config_hash)?;

		// Relink the pages whose last link had errors, and those that use an affected
		// symbol.
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
		Ok(())
	}

	fn remove(&mut self, path: &str) -> Result<(), BuildError> {
		let Some(unit) = self.index.unit(path)? else {
			return Ok(());
		};
		for change in self.index.remove(path)? {
			self.affected.insert((unit.name.clone(), change.name));
		}
		self.affected.insert((unit.name, String::new()));
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
		let failed = has_errors(&compiled.diagnostics);
		self.report.diagnostics.extend(compiled.diagnostics);
		if failed {
			self.report.failed.push(file.path.clone());
			return Ok(());
		}

		let unit = compiled.unit;
		let document = &unit.document;
		let basename = file.path.rsplit('/').next().unwrap();
		let basename = basename.strip_suffix(".nml").unwrap_or(basename);
		let name = unit.reference_key.as_deref().unwrap_or(basename);
		let title = title(document).unwrap_or_else(|| basename.to_string());
		let template = (file.role == Role::Page).then(|| {
			let options = PageOptions {
				language: self.config.html.language.clone(),
				title: title.clone(),
				stylesheets: self.config.html.stylesheets.clone(),
				icon: self.config.html.icon.clone(),
			};
			store_template(document, &nml_html::compile(document, &options))
		});
		let uses: Vec<_> = unit.uses.iter().map(store_use).collect();

		let old = self.index.unit(&file.path)?;
		let old_exports = match &old {
			Some(old) if old.name != name => self.index.exports(old.id)?,
			_ => Vec::new(),
		};
		let (_, changes) = self.index.commit(&StoredUnit {
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
		for change in changes {
			self.affected.insert((name.to_string(), change.name));
		}
		// What dependents see of the unit itself: its key, title and role. If they changed,
		// every reference to the unit may resolve differently.
		let identity_changed = old
			.as_ref()
			.is_none_or(|old| old.name != name || old.title != title || old.role != file.role);
		if identity_changed {
			self.affected.insert((name.to_string(), String::new()));
			for export in &unit.exports {
				self.affected
					.insert((name.to_string(), export.name.clone()));
			}
		}
		if let Some(old) = old.filter(|old| old.name != name) {
			self.affected.insert((old.name.clone(), String::new()));
			for export in old_exports {
				self.affected.insert((old.name.clone(), export.name));
			}
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
		let target = self.output.join(output_path(path));
		let io_error = |e| BuildError::Io(target.clone(), e);
		if let Some(parent) = target.parent() {
			fs::create_dir_all(parent).map_err(|e| BuildError::Io(parent.into(), e))?;
		}
		let mut file = io::BufWriter::new(fs::File::create(&target).map_err(io_error)?);

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
				let Slot::Link(link) = &stored.slot else {
					// Deferred slots are filled by Lua (Phase 4).
					return Resolved::Value(String::new());
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

/// Finds included files and imported modules on disk.
struct WorkspaceResolver<'a> {
	root: &'a Path,
	config: &'a Config,
}

impl Resolver for WorkspaceResolver<'_> {
	fn include(&self, from: &str, path: &str) -> Result<(String, Arc<Source>), ResolveError> {
		let not_found = |path: &str| ResolveError::NotFound { path: path.into() };
		let resolved = resolve_path(from, path).ok_or_else(|| not_found(path))?;
		let absolute = self.root.join(&resolved);
		let content = fs::read_to_string(&absolute).map_err(|_| not_found(&resolved))?;
		Ok((resolved, source(&absolute, content)))
	}

	fn module(&self, module: &str) -> Result<String, ResolveError> {
		let (name, rest) = module.split_once('.').unwrap_or((module, ""));
		let root = self
			.config
			.modules
			.iter()
			.find(|m| m.name == name)
			.ok_or_else(|| ResolveError::UnknownModule { name: name.into() })?;
		let root: Vec<_> = root
			.root
			.components()
			.filter_map(|c| match c {
				std::path::Component::Normal(part) => Some(part.to_string_lossy()),
				_ => None,
			})
			.collect();
		let path = format!("{}/{}.nml", root.join("/"), rest.replace('.', "/"));
		if self.root.join(&path).is_file() {
			Ok(path)
		} else {
			Err(ResolveError::NotFound { path })
		}
	}
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
