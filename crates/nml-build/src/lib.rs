//! The build pipeline shared by the CLI and the language server.

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_diagnostics::DiagnosticSeverity;
use nml_document::Document;
use nml_document::ElementKind;
use nml_document::Slot;
use nml_html::PageOptions;
use nml_html::Resolved;
use nml_lang::Language;
use nml_source::Source;
use nml_source::Url;
use nml_workspace::CONFIG_FILE;
use nml_workspace::Config;

/// The result of a build.
#[derive(Debug, Default)]
pub struct BuildReport {
	/// Pages written.
	pub written: Vec<PathBuf>,
	/// Pages not written because of errors.
	pub failed: Vec<PathBuf>,
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

/// A build failure that is not about a source: a missing configuration, an I/O error.
#[derive(Debug)]
pub enum BuildError {
	NoConfig(PathBuf),
	Io(PathBuf, io::Error),
}

impl std::fmt::Display for BuildError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			BuildError::NoConfig(root) => {
				write!(f, "no {CONFIG_FILE} in {}", root.display())
			}
			BuildError::Io(path, error) => write!(f, "{}: {error}", path.display()),
		}
	}
}

impl std::error::Error for BuildError {}

fn read(path: &Path) -> Result<Arc<Source>, BuildError> {
	let content = fs::read_to_string(path).map_err(|e| BuildError::Io(path.into(), e))?;
	let url = Url::from_file_path(path).expect("workspace paths are absolute");
	Ok(Arc::new(Source::new(url, content, 0)))
}

/// Builds every page of the workspace at `root`, without the database: a full build.
pub fn build(root: &Path) -> Result<BuildReport, BuildError> {
	let root = fs::canonicalize(root).map_err(|e| BuildError::Io(root.into(), e))?;
	let config_path = root.join(CONFIG_FILE);
	if !config_path.exists() {
		return Err(BuildError::NoConfig(root));
	}
	let mut report = BuildReport::default();
	let config = match Config::parse(read(&config_path)?) {
		Ok(config) => config,
		Err(diagnostics) => {
			report.diagnostics = diagnostics;
			return Ok(report);
		}
	};

	let output = root.join(&config.output);
	let skip = [output.clone(), root.join(".nml")];
	let mut files = Vec::new();
	collect(&root, &skip, &mut files)?;
	files.sort();

	let language = Language::builtin();
	for path in files {
		let relative = path.strip_prefix(&root).unwrap();
		if !config.is_page(relative) {
			continue;
		}
		let compiled = nml_lang::compile(&language, read(&path)?);
		let failed = has_errors(&compiled.diagnostics);
		report.diagnostics.extend(compiled.diagnostics);
		if failed {
			report.failed.push(relative.into());
			continue;
		}

		let document = &compiled.unit.document;
		let options = PageOptions {
			language: config.html.language.clone(),
			title: title(document, relative),
			stylesheets: config.html.stylesheets.clone(),
			icon: config.html.icon.clone(),
		};
		let template = nml_html::compile(document, &options);
		let target = output.join(relative).with_extension("html");
		if let Some(parent) = target.parent() {
			fs::create_dir_all(parent).map_err(|e| BuildError::Io(parent.into(), e))?;
		}
		let mut file = io::BufWriter::new(
			fs::File::create(&target).map_err(|e| BuildError::Io(target.clone(), e))?,
		);
		// References are not resolved yet: every slot is reported as unresolved.
		let diagnostics = nml_html::write(
			&template,
			|slot| Resolved::Missing(unresolved(document, slot.slot)),
			&mut file,
		)
		.map_err(|e| BuildError::Io(target.clone(), e))?;
		report.diagnostics.extend(diagnostics);
		report.written.push(relative.into());
	}
	Ok(report)
}

/// The `.nml` files under `dir`, except in the `skip` directories and hidden ones.
fn collect(dir: &Path, skip: &[PathBuf], files: &mut Vec<PathBuf>) -> Result<(), BuildError> {
	let entries = fs::read_dir(dir).map_err(|e| BuildError::Io(dir.into(), e))?;
	for entry in entries {
		let entry = entry.map_err(|e| BuildError::Io(dir.into(), e))?;
		let path = entry.path();
		let hidden = entry.file_name().to_string_lossy().starts_with('.');
		let file_type = entry
			.file_type()
			.map_err(|e| BuildError::Io(path.clone(), e))?;
		if file_type.is_dir() && !hidden && !skip.contains(&path) {
			collect(&path, skip, files)?;
		} else if file_type.is_file() && path.extension().is_some_and(|e| e == "nml") {
			files.push(path);
		}
	}
	Ok(())
}

/// The page title: the text of its first heading, or the file name.
fn title(document: &Document, path: &Path) -> String {
	document
		.descendants(document.root())
		.find(|(_, element)| matches!(element.kind, ElementKind::Heading { .. }))
		.map(|(id, _)| document.text(id))
		.unwrap_or_else(|| path.file_stem().unwrap().to_string_lossy().into_owned())
}

fn unresolved(document: &Document, slot: nml_document::SlotId) -> Diagnostic {
	let target = match document.slot(slot) {
		Slot::Link(link) => link.target.to_string(),
		_ => String::new(),
	};
	let token = document
		.descendants(document.root())
		.find(|(_, element)| match element.kind {
			ElementKind::Slot(id) | ElementKind::Reference { href: id } => id == slot,
			_ => false,
		})
		.map(|(_, element)| element.token.clone())
		.unwrap();
	let kind = DiagnosticKind::UnresolvedReference { target };
	Diagnostic::new(kind, DiagnosticMessage::new(token))
}

#[cfg(test)]
mod tests {
	use super::*;

	fn workspace(files: &[(&str, &str)]) -> PathBuf {
		let dir = std::env::temp_dir().join(format!(
			"nml-build-test-{}-{}",
			std::process::id(),
			files.len()
		));
		let _ = fs::remove_dir_all(&dir);
		for (path, content) in files {
			let path = dir.join(path);
			fs::create_dir_all(path.parent().unwrap()).unwrap();
			fs::write(path, content).unwrap();
		}
		dir
	}

	#[test]
	fn builds_pages() {
		let root = workspace(&[
			(
				"nml.toml",
				"exclude = [\"lib/**\"]\n[html]\nlanguage = \"en\"\n",
			),
			("index.nml", "# Welcome {#top}\n\nSome **bold** text.\n"),
			("docs/guide.nml", "A *guide*."),
			("lib/util.nml", "not a page"),
			("broken.nml", "**unclosed"),
			(".hidden/x.nml", "hidden"),
		]);
		let report = build(&root).unwrap();

		assert_eq!(
			report.written,
			[PathBuf::from("docs/guide.nml"), PathBuf::from("index.nml")]
		);
		assert_eq!(report.failed, [PathBuf::from("broken.nml")]);
		assert_eq!(report.diagnostics.len(), 1);

		let index = fs::read_to_string(root.join("public/index.html")).unwrap();
		assert!(index.contains("<html lang=\"en\">"));
		assert!(index.contains("<title>Welcome</title>"));
		assert!(index.contains("<h1 id=\"top\">Welcome</h1>"));
		assert!(index.contains("<p>Some <strong>bold</strong> text.</p>"));
		let guide = fs::read_to_string(root.join("public/docs/guide.html")).unwrap();
		assert!(guide.contains("<title>guide</title>"));
		assert!(guide.contains("<p>A <em>guide</em>.</p>"));
		assert!(!root.join("public/broken.html").exists());

		fs::remove_dir_all(root).unwrap();
	}

	#[test]
	fn needs_a_config() {
		let root = workspace(&[("a.nml", "x")]);
		assert!(matches!(build(&root), Err(BuildError::NoConfig(_))));
		fs::remove_dir_all(root).unwrap();
	}
}
