use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use globset::Glob;
use globset::GlobSet;
use globset::GlobSetBuilder;
use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_source::Source;
use nml_source::Token;
use serde::Deserialize;
use toml::Spanned;

/// The name of the configuration file, at the root of a workspace.
pub const CONFIG_FILE: &str = "nml.toml";

/// A workspace's `nml.toml`.
#[derive(Debug, Clone)]
pub struct Config {
	/// Patterns of the files that are pages, relative to the workspace root. Every other
	/// `.nml` file is a library.
	pub pages: Vec<String>,
	pub exclude: Vec<String>,
	page_set: GlobSet,
	exclude_set: GlobSet,
	/// The output directory, relative to the workspace root.
	pub output: PathBuf,
	/// The workspace database, relative to the workspace root.
	pub database: PathBuf,
	pub html: HtmlConfig,
	pub modules: Vec<Module>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HtmlConfig {
	/// The `lang` of every page.
	pub language: Option<String>,
	/// Stylesheet URLs, linked from every page.
	#[serde(default)]
	pub stylesheets: Vec<String>,
	pub icon: Option<String>,
}

/// A `[[module]]` section: `:import name.a.b` resolves to `<root>/a/b.nml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
	pub name: String,
	/// Relative to the workspace root.
	pub root: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
	#[serde(default)]
	pages: Option<Vec<Spanned<String>>>,
	#[serde(default)]
	exclude: Vec<Spanned<String>>,
	output: Option<PathBuf>,
	database: Option<PathBuf>,
	#[serde(default)]
	html: HtmlConfig,
	#[serde(default, rename = "module")]
	modules: Vec<RawModule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawModule {
	name: Spanned<String>,
	root: PathBuf,
}

impl Config {
	/// Parses and checks the content of `nml.toml`.
	pub fn parse(source: Arc<Source>) -> Result<Self, Vec<Diagnostic>> {
		let token = |range| Token::new(source.clone(), range);
		let raw: Raw = toml::from_str(source.content()).map_err(|error| {
			let kind = DiagnosticKind::ConfigError {
				message: error.message().to_string(),
			};
			let location = token(error.span().unwrap_or(0..0));
			vec![Diagnostic::new(kind, DiagnosticMessage::new(location))]
		})?;

		let mut diagnostics = Vec::new();
		let default_pages = vec![Spanned::new(0..0, "**/*.nml".to_string())];
		let pages = raw.pages.unwrap_or(default_pages);
		let mut globs = |patterns: &[Spanned<String>]| {
			let mut set = GlobSetBuilder::new();
			for pattern in patterns {
				match Glob::new(pattern.get_ref()) {
					Ok(glob) => {
						set.add(glob);
					}
					Err(error) => {
						let kind = DiagnosticKind::InvalidGlob {
							pattern: pattern.get_ref().clone(),
							message: error.kind().to_string(),
						};
						let location = DiagnosticMessage::new(token(pattern.span()));
						diagnostics.push(Diagnostic::new(kind, location));
					}
				}
			}
			set.build().unwrap_or_else(|_| GlobSet::empty())
		};
		let page_set = globs(&pages);
		let exclude_set = globs(&raw.exclude);

		let mut modules: Vec<(Module, Token)> = Vec::new();
		for module in raw.modules {
			let name = module.name.get_ref();
			let location = token(module.name.span());
			if !is_module_name(name) {
				let kind = DiagnosticKind::InvalidModuleName { name: name.clone() };
				diagnostics.push(Diagnostic::new(kind, DiagnosticMessage::new(location)));
				continue;
			}
			if let Some((_, first)) = modules.iter().find(|(m, _)| m.name == *name) {
				let kind = DiagnosticKind::DuplicateModule { name: name.clone() };
				let diagnostic = Diagnostic::new(kind, DiagnosticMessage::new(location))
					.with(DiagnosticMessage::fluent(first.clone(), "first"));
				diagnostics.push(diagnostic);
				continue;
			}
			let module = Module {
				name: name.clone(),
				root: module.root,
			};
			modules.push((module, location));
		}

		if !diagnostics.is_empty() {
			return Err(diagnostics);
		}
		Ok(Self {
			pages: pages.into_iter().map(Spanned::into_inner).collect(),
			exclude: raw.exclude.into_iter().map(Spanned::into_inner).collect(),
			page_set,
			exclude_set,
			output: raw.output.unwrap_or_else(|| "public".into()),
			database: raw.database.unwrap_or_else(|| ".nml/index.db".into()),
			html: raw.html,
			modules: modules.into_iter().map(|(module, _)| module).collect(),
		})
	}

	/// Whether a file, relative to the workspace root, is a page.
	pub fn is_page(&self, path: &Path) -> bool {
		self.page_set.is_match(path) && !self.exclude_set.is_match(path)
	}
}

/// A module name is a word: it is the first part of an import path.
fn is_module_name(name: &str) -> bool {
	let mut chars = name.chars();
	chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
		&& chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_source::Url;

	use super::*;

	fn parse(text: &str) -> Result<Config, Vec<Diagnostic>> {
		let url = Url::parse("file:///w/nml.toml").unwrap();
		Config::parse(Arc::new(Source::new(url, text.into(), 0)))
	}

	#[test]
	fn defaults() {
		let config = parse("").unwrap();
		assert_eq!(config.pages, ["**/*.nml"]);
		assert_eq!(config.output, Path::new("public"));
		assert_eq!(config.database, Path::new(".nml/index.db"));
		assert!(config.modules.is_empty());
		assert!(config.is_page(Path::new("a/b.nml")));
		assert!(!config.is_page(Path::new("a/b.txt")));
	}

	#[test]
	fn full() {
		let config = parse(
			r#"
pages = ["pages/**/*.nml", "index.nml"]
exclude = ["pages/drafts/**"]
output = "site"
database = "index.db"

[html]
language = "en"
stylesheets = ["style.css"]
icon = "icon.png"

[[module]]
name = "std"
root = "modules/std"

[[module]]
name = "nav"
root = "lib/nav"
"#,
		)
		.unwrap();
		assert!(config.is_page(Path::new("pages/a/b.nml")));
		assert!(config.is_page(Path::new("index.nml")));
		assert!(!config.is_page(Path::new("pages/drafts/x.nml")));
		assert!(!config.is_page(Path::new("lib/nav/menu.nml")));
		assert_eq!(config.output, Path::new("site"));
		assert_eq!(config.html.language.as_deref(), Some("en"));
		assert_eq!(config.html.stylesheets, ["style.css"]);
		assert_eq!(
			config.modules,
			[
				Module {
					name: "std".into(),
					root: "modules/std".into()
				},
				Module {
					name: "nav".into(),
					root: "lib/nav".into()
				}
			]
		);
	}

	#[test]
	fn errors() {
		let diagnostics = parse("pages = [\"a\"\n").unwrap_err();
		assert_diagnostics!(diagnostics, [3002]);

		let diagnostics = parse("output = \"x\"\ncolour = 1\n").unwrap_err();
		assert_diagnostics!(diagnostics, [3002 @ 13..19]);

		let diagnostics = parse(
			"pages = [\"a/{b\"]\n[[module]]\nname = \"a.b\"\nroot = \"x\"\n\
			[[module]]\nname = \"m\"\nroot = \"x\"\n[[module]]\nname = \"m\"\nroot = \"y\"\n",
		)
		.unwrap_err();
		assert_diagnostics!(diagnostics, [3003 @ 9..15, 3004 @ 35..40, 3005 @ 103..106]);
	}
}
