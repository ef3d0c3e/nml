use std::sync::Arc;

use parking_lot::RwLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::compiler::sanitize::Sanitizer;
use crate::parser::reports::macros::*;
use crate::parser::reports::Report;
use crate::parser::reports::*;
use crate::parser::source::Token;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::scope::Scope;
use lazy_static::lazy_static;

#[derive(Debug, Clone)]
pub struct CodeDisplay {
	pub title: Option<String>,
	pub line_gutter: bool,
	pub line_offset: usize,
	pub inline: bool,
	pub theme: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Code {
	pub(crate) location: Token,
	pub(crate) language: String,
	pub(crate) display: CodeDisplay,
	pub(crate) content: String,
}

impl Code {
	pub fn syntaxes() -> &'static SyntaxSet {
		lazy_static! {
			static ref set: SyntaxSet = SyntaxSet::load_defaults_newlines();
		}
		&set
	}

	fn highlight_html(&self, sanitizer: &Sanitizer) -> Result<String, String> {
		lazy_static! {
			static ref theme_set: ThemeSet = ThemeSet::load_defaults();
		}

		let syntax = match Self::syntaxes().find_syntax_by_name(self.language.as_str()) {
			Some(syntax) => syntax,
			None => {
				return Err(format!(
					"Unable to find syntax for language: {}",
					self.language
				))
			}
		};

		let theme = self
			.display
			.theme
			.as_ref()
			.map_or("base16-ocean.dark", |theme| theme.as_str());
		let mut highlight = HighlightLines::new(syntax, &theme_set.themes[theme]);

		let mut result = String::new();
		if self.display.inline {
			result += "<pre class=\"inline-code\"><code>";
			match highlight.highlight_line(self.content.as_str(), Code::syntaxes()) {
				Err(e) => return Err(format!("Error highlighting line `{}`: {}", self.content, e)),
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
			result += "</code></pre>";
		} else {
			result += "<div class=\"code-block\">";
			// Output title if some
			if let Some(title) = self.display.title.as_ref() {
				if !title.is_empty() {
					result += format!(
						"<div class=\"code-block-title\">{}</div>",
						sanitizer.sanitize(title.as_str())
					)
					.as_str();
				}
			}

			if self.display.line_gutter {
				result += format!(
					"<pre><code class=\"line-gutter\" style=\"--line-offset:{}\">",
					self.display.line_offset
				)
				.as_str();
			} else {
				result += "<pre><code>";
			}

			// Highlight content
			let content = self.content.trim_end();
			for (line_num, line) in content.split('\n').enumerate() {
				match highlight.highlight_line(line, Code::syntaxes()) {
					Err(e) => return Err(format!("Error highlighting line `{line}`: {}", e)),
					Ok(regions) => {
						match syntect::html::styled_line_to_highlighted_html(
							&regions[..],
							syntect::html::IncludeBackground::No,
						) {
							Err(e) => return Err(format!("Error highlighting code: {}", e)),
							Ok(highlighted) => {
								result += "<span>";
								result += highlighted.as_str();
								result += "</span>\n";
							}
						}
					}
				}
			}
			result += "</code></pre></div>";
		}
		Ok(result)
	}
}

impl Element for Code {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> crate::unit::element::ElemKind {
		if self.display.inline {
			ElemKind::Inline
		} else {
			ElemKind::Block
		}
	}

	fn element_name(&self) -> &'static str {
		"Code"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		assert_eq!(compiler.target(), crate::compiler::compiler::Target::HTML);

		let value = self.clone();
		let sanitizer = compiler.sanitizer();

		let fut = async move {
			match value.highlight_html(&sanitizer) {
				Ok(result) => Ok(result),
				Err(e) => Err(compile_err!(
					value.location,
					"Failed to process Graphviz element".to_string(),
					e
				)),
			}
		};
		output.add_task(self.location.clone(), "Code".into(), Box::pin(fut));
		Ok(())
	}
}
