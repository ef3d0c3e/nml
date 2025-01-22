use std::sync::Once;

use crate::cache::cache::Cached;
use crate::cache::cache::CachedError;
use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::CompilerOutput;
use crate::compiler::compiler::Target::HTML;
use crate::compiler::sanitize::Sanitizer;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use compiler::compiler;
use crypto::digest::Digest;
use crypto::sha2::Sha512;
use lazy_static::lazy_static;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeKind {
	FullBlock,
	MiniBlock,
	Inline,
}

impl From<&CodeKind> for ElemKind {
	fn from(value: &CodeKind) -> Self {
		match value {
			CodeKind::FullBlock | CodeKind::MiniBlock => ElemKind::Block,
			CodeKind::Inline => ElemKind::Inline,
		}
	}
}

#[derive(Debug, Clone)]
pub struct Code {
	pub location: Token,
	pub block: CodeKind,
	pub language: String,
	pub name: Option<String>,
	pub code: String,
	pub theme: Option<String>,
	pub line_offset: usize,
}

impl Code {
	pub fn get_syntaxes() -> &'static SyntaxSet {
		lazy_static! {
			static ref syntax_set: SyntaxSet = SyntaxSet::load_defaults_newlines();
		}

		&syntax_set
	}

	fn highlight_html(&self, sanitizer: &Sanitizer) -> Result<String, String> {
		lazy_static! {
			static ref theme_set: ThemeSet = ThemeSet::load_defaults();
		}
		let syntax = match Code::get_syntaxes().find_syntax_by_name(self.language.as_str()) {
			Some(syntax) => syntax,
			None => {
				return Err(format!(
					"Unable to find syntax for language: {}",
					self.language
				))
			}
		};

		let theme_string = match self.theme.as_ref() {
			Some(theme) => theme.as_str(),
			None => "base16-ocean.dark",
		};
		let mut h = HighlightLines::new(syntax, &theme_set.themes[theme_string]);

		let mut result = String::new();
		if self.block == CodeKind::FullBlock {
			result += "<div class=\"code-block\">";
			if let Some(name) = &self.name {
				result += format!(
					"<div class=\"code-block-title\">{}</div>",
					sanitizer.sanitize(name.as_str())
				)
				.as_str();
			}

			result += "<div class=\"code-block-content\"><table class=\"code-block-table\" cellspacing=\"0\">"
				.to_string()
				.as_str();
			for (line_id, line) in self.code.split('\n').enumerate() {
				result += "<tr><td class=\"code-block-gutter\">";

				// Line number
				result +=
					format!("<pre><span>{}</span></pre>", line_id + self.line_offset).as_str();

				// Code
				result += "</td><td class=\"code-block-line\"><pre>";
				match h.highlight_line(line, Code::get_syntaxes()) {
					Err(e) => return Err(format!("Error highlighting line `{line}`: {}", e)),
					Ok(regions) => {
						match syntect::html::styled_line_to_highlighted_html(
							&regions[..],
							syntect::html::IncludeBackground::No,
						) {
							Err(e) => return Err(format!("Error highlighting code: {}", e)),
							Ok(highlighted) => {
								result += if highlighted.is_empty() {
									"<br>"
								} else {
									highlighted.as_str()
								}
							}
						}
					}
				}
				result += "</pre></td></tr>";
			}

			result += "</table></div></div>";
		} else if self.block == CodeKind::MiniBlock {
			result += "<div class=\"code-block\"><div class=\"code-block-content\"><table class=\"code-block-table\" cellspacing=\"0\">";

			for line in self.code.split('\n') {
				result += "<tr><td class=\"code-block-line\"><pre>";
				// Code
				match h.highlight_line(line, Code::get_syntaxes()) {
					Err(e) => return Err(format!("Error highlighting line `{line}`: {}", e)),
					Ok(regions) => {
						match syntect::html::styled_line_to_highlighted_html(
							&regions[..],
							syntect::html::IncludeBackground::No,
						) {
							Err(e) => return Err(format!("Error highlighting code: {}", e)),
							Ok(highlighted) => {
								result += if highlighted.is_empty() {
									"<br>"
								} else {
									highlighted.as_str()
								}
							}
						}
					}
				}
				result += "</pre></td></tr>";
			}
			result += "</table></div></div>";
		} else if self.block == CodeKind::Inline {
			result += "<a class=\"inline-code\"><code>";
			match h.highlight_line(self.code.as_str(), Code::get_syntaxes()) {
				Err(e) => return Err(format!("Error highlighting line `{}`: {}", self.code, e)),
				Ok(regions) => {
					match syntect::html::styled_line_to_highlighted_html(
						&regions[..],
						syntect::html::IncludeBackground::No,
					) {
						Err(e) => return Err(format!("Error highlighting code: {}", e)),
						Ok(highlighted) => result += highlighted.as_str(),
					}
				}
			}
			result += "</code></a>";
		}

		Ok(result)
	}
}

impl Cached for Code {
	type Key = String;
	type Value = String;

	fn sql_table() -> &'static str {
		"CREATE TABLE IF NOT EXISTS cached_code (
				digest	     TEXT PRIMARY KEY,
				highlighted  BLOB NOT NULL);"
	}

	fn sql_get_query() -> &'static str { "SELECT highlighted FROM cached_code WHERE digest = (?1)" }

	fn sql_insert_query() -> &'static str {
		"INSERT INTO cached_code (digest, highlighted) VALUES (?1, ?2)"
	}

	fn key(&self) -> <Self as Cached>::Key {
		let mut hasher = Sha512::new();
		hasher.input((self.block as usize).to_be_bytes().as_slice());
		hasher.input(self.line_offset.to_be_bytes().as_slice());
		if let Some(theme) = self.theme.as_ref() {
			hasher.input(theme.as_bytes())
		}
		if let Some(name) = self.name.as_ref() {
			hasher.input(name.as_bytes())
		}
		hasher.input(self.language.as_bytes());
		hasher.input(self.code.as_bytes());

		hasher.result_str()
	}
}

impl Element for Code {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { (&self.block).into() }

	fn element_name(&self) -> &'static str { "Code Block" }

	fn compile<'e>(
		&'e self,
		compiler: &'e Compiler,
		_document: &dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				static CACHE_INIT: Once = Once::new();
				let cache = compiler.cache();

				CACHE_INIT.call_once(|| {
					let con = tokio::runtime::Runtime::new().unwrap().block_on(cache.get_connection());

					if let Err(e) = Code::init(&con) {
						eprintln!("Unable to create cache table: {e}");
					}
				});

				let sanitizer = compiler.sanitizer();
				let code = self.clone();
				let fut = async move {
					let con = cache.get_connection().await;
					match code.cached(&con, |s| s.highlight_html(&sanitizer)) {
						Ok(s) => Ok(s),
						Err(e) => match e {
							CachedError::SqlErr(e) => Err(compile_err!(code.location(), "Failed to compile code".into(), format!("Querying the cache failed: {e}"))),
							CachedError::GenErr(e) => Err(compile_err!(code.location(), "Failed to compile code".into(), e)),
						},
					}
				};
				output.add_task(Box::pin(fut));
			}
			_ => todo!(""),
		}
		Ok(())
	}
}
