use std::sync::Arc;

use auto_userdata::AutoUserData;
use mlua::AnyUserData;
use mlua::Lua;
use mlua::LuaSerdeExt;
use parking_lot::RwLock;
use serde::Deserialize;
use serde::Serialize;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeDisplay {
	pub title: Option<String>,
	pub line_gutter: bool,
	pub line_offset: usize,
	pub inline: bool,
	pub max_lines: Option<usize>,
	pub theme: Option<String>,
}

#[derive(Debug, Clone, AutoUserData)]
#[auto_userdata_target = "*"]
#[auto_userdata_target = "&"]
#[auto_userdata_target = "&mut"]
pub struct Code {
	pub(crate) location: Token,
	pub(crate) language: String,
	#[lua_value]
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
			result += r#"<code class="inline-code">"#;
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
			result += "</code>";
		} else {
			result += "<figure class=\"code-block\">";
			// Output title if some
			if let Some(title) = self.display.title.as_ref() {
				if !title.is_empty() {
					result += format!(
						"<figcaption class=\"code-block-title\">{}</figcaption>",
						sanitizer.sanitize(title.as_str())
					)
					.as_str();
				}
			}

			if self.display.line_gutter {
				let max_height = if let Some(lines) = self.display.max_lines {
					format!(";max-height:calc({lines}*var(--line-height))")
				} else {
					"".into()
				};
				result += &format!(
					"<pre><code class=\"line-gutter\" style=\"--line-offset:{}{max_height}\">",
					self.display.line_offset
				);
			} else {
				let max_height = if let Some(lines) = self.display.max_lines {
					format!(r#" style="max-height:calc({lines}*var(--line-height))""#)
				} else {
					"".into()
				};
				result += &format!("<pre><code{max_height}>");
			}

			// Highlight content
			let content = self.content.trim_end();
			for line in content.split('\n') {
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
			result += "</code></pre></figure>";
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

	fn provide_hover(&self) -> Option<String> {
		let mut hover = String::default();
		hover += format!(
			"Code Fragment

# Properties
 * **Location**: [{}] ({}..{})
 * **Language**: {}",
			self.location.source().name().display(),
			self.location().range.start,
			self.location().range.end,
			self.language,
		)
		.as_str();
		if let Some(title) = &self.display.title {
			hover += format!("\n * **Title**: {title}").as_str();
		}
		if self.display.line_offset != 0 {
			hover += format!("\n * **Line Offset**: {}", self.display.line_offset).as_str();
		}
		if let Some(theme) = &self.display.theme {
			hover += format!("\n * **Theme**: {theme}").as_str();
		}
		hover += format!(
			"\n * **Display**: *{}* + *{}*",
			if self.display.inline {
				"Inline"
			} else {
				"Block"
			},
			if self.display.line_gutter {
				"Line Gutter"
			} else {
				"No Line Gutter"
			}
		)
		.as_str();
		Some(hover)
	}

	fn lua_wrap(self: Arc<Self>, lua: &Lua) -> Option<AnyUserData> {
		let r: &'static _ = unsafe { &*Arc::as_ptr(&self) };
		Some(lua.create_userdata(r).unwrap())
	}
}
