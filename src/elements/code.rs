use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Once;

use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use crypto::digest::Digest;
use crypto::sha2::Sha512;
use mlua::Function;
use mlua::Lua;
use regex::Captures;
use regex::Regex;
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

use crate::cache::cache::Cached;
use crate::cache::cache::CachedError;
use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::lua::kernel::CTX;
use crate::parser::parser::ParserState;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::util::Property;
use crate::parser::util::PropertyMapError;
use crate::parser::util::PropertyParser;
use crate::parser::util::{self};
use lazy_static::lazy_static;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodeKind {
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

#[derive(Debug)]
struct Code {
	location: Token,
	block: CodeKind,
	language: String,
	name: Option<String>,
	code: String,
	theme: Option<String>,
	line_offset: usize,
}

impl Code {
	fn new(
		location: Token,
		block: CodeKind,
		language: String,
		name: Option<String>,
		code: String,
		theme: Option<String>,
		line_offset: usize,
	) -> Self {
		Self {
			location,
			block,
			language,
			name,
			code,
			theme,
			line_offset,
		}
	}

	pub fn get_syntaxes() -> &'static SyntaxSet {
		lazy_static! {
			static ref syntax_set: SyntaxSet = SyntaxSet::load_defaults_newlines();
		}

		&syntax_set
	}

	fn highlight_html(&self, compiler: &Compiler) -> Result<String, String> {
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
					Compiler::sanitize(compiler.target(), name.as_str())
				)
				.as_str();
			}

			result +=
				format!("<div class=\"code-block-content\"><table cellspacing=\"0\">").as_str();
			for (line_id, line) in self.code.split(|c| c == '\n').enumerate() {
				result += "<tr><td class=\"code-block-gutter\">";

				// Line number
				result +=
					format!("<pre><span>{}</span></pre>", line_id + self.line_offset).as_str();

				// Code
				result += "</td><td class=\"code-block-line\"><pre>";
				match h.highlight_line(line, Code::get_syntaxes()) {
					Err(e) => {
						return Err(format!(
							"Error highlighting line `{line}`: {}",
							e.to_string()
						))
					}
					Ok(regions) => {
						match syntect::html::styled_line_to_highlighted_html(
							&regions[..],
							syntect::html::IncludeBackground::No,
						) {
							Err(e) => {
								return Err(format!("Error highlighting code: {}", e.to_string()))
							}
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
			result += "<div class=\"code-block\"><div class=\"code-block-content\"><table cellspacing=\"0\">";

			for line in self.code.split(|c| c == '\n') {
				result += "<tr><td class=\"code-block-line\"><pre>";
				// Code
				match h.highlight_line(line, Code::get_syntaxes()) {
					Err(e) => {
						return Err(format!(
							"Error highlighting line `{line}`: {}",
							e.to_string()
						))
					}
					Ok(regions) => {
						match syntect::html::styled_line_to_highlighted_html(
							&regions[..],
							syntect::html::IncludeBackground::No,
						) {
							Err(e) => {
								return Err(format!("Error highlighting code: {}", e.to_string()))
							}
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
				Err(e) => {
					return Err(format!(
						"Error highlighting line `{}`: {}",
						self.code,
						e.to_string()
					))
				}
				Ok(regions) => {
					match syntect::html::styled_line_to_highlighted_html(
						&regions[..],
						syntect::html::IncludeBackground::No,
					) {
						Err(e) => {
							return Err(format!("Error highlighting code: {}", e.to_string()))
						}
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
		hasher.input((self.line_offset as usize).to_be_bytes().as_slice());
		self.theme
			.as_ref()
			.map(|theme| hasher.input(theme.as_bytes()));
		self.name.as_ref().map(|name| hasher.input(name.as_bytes()));
		hasher.input(self.language.as_bytes());
		hasher.input(self.code.as_bytes());

		hasher.result_str()
	}
}

impl Element for Code {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { (&self.block).into() }

	fn element_name(&self) -> &'static str { "Code Block" }

	fn compile(&self, compiler: &Compiler, _document: &dyn Document) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				static CACHE_INIT: Once = Once::new();
				CACHE_INIT.call_once(|| {
					if let Some(mut con) = compiler.cache() {
						if let Err(e) = Code::init(&mut con) {
							eprintln!("Unable to create cache table: {e}");
						}
					}
				});

				if let Some(mut con) = compiler.cache() {
					match self.cached(&mut con, |s| s.highlight_html(compiler)) {
						Ok(s) => Ok(s),
						Err(e) => match e {
							CachedError::SqlErr(e) => {
								Err(format!("Querying the cache failed: {e}"))
							}
							CachedError::GenErr(e) => Err(e),
						},
					}
				} else {
					self.highlight_html(compiler)
				}
			}
			Target::LATEX => {
				todo!("")
			}
		}
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::code")]
pub struct CodeRule {
	re: [Regex; 2],
	properties: PropertyParser,
}

impl CodeRule {
	pub fn new() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"line_offset".to_string(),
			Property::new(
				true,
				"Line number offset".to_string(),
				Some("1".to_string()),
			),
		);
		Self {
			re: [
				Regex::new(
					r"(?:^|\n)```(?:\[((?:\\.|[^\\\\])*?)\])?(.*?)(?:,(.*))?\n((?:\\(?:.|\n)|[^\\\\])*?)```",
				)
				.unwrap(),
				Regex::new(
					r"``(?:\[((?:\\.|[^\\\\])*?)\])?(?:(.*?),)?((?:\\(?:.|\n)|[^\\\\])*?)``",
				)
				.unwrap(),
			],
			properties: PropertyParser { properties: props },
		}
	}
}

impl RegexRule for CodeRule {
	fn name(&self) -> &'static str { "Code" }
	fn previous(&self) -> Option<&'static str> { Some("List") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn on_regex_match<'a>(
		&self,
		index: usize,
		state: &ParserState,
		document: &'a dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let properties = match matches.get(1) {
			None => match self.properties.default() {
				Ok(properties) => properties,
				Err(e) => {
					reports.push(
						Report::build(ReportKind::Error, token.source(), token.start())
							.with_message("Invalid code")
							.with_label(
								Label::new((token.source().clone(), token.range.clone()))
									.with_message(format!("Code is missing properties: {e}"))
									.with_color(state.parser.colors().error),
							)
							.finish(),
					);
					return reports;
				}
			},
			Some(props) => {
				let processed =
					util::process_escaped('\\', "]", props.as_str().trim_start().trim_end());
				match self.properties.parse(processed.as_str()) {
					Err(e) => {
						reports.push(
							Report::build(ReportKind::Error, token.source(), props.start())
								.with_message("Invalid Code Properties")
								.with_label(
									Label::new((token.source().clone(), props.range()))
										.with_message(e)
										.with_color(state.parser.colors().error),
								)
								.finish(),
						);
						return reports;
					}
					Ok(properties) => properties,
				}
			}
		};

		let code_lang = match matches.get(2) {
			None => "Plain Text".to_string(),
			Some(lang) => {
				let code_lang = lang.as_str().trim_start().trim_end().to_string();
				if code_lang.is_empty() {
					reports.push(
						Report::build(ReportKind::Error, token.source(), lang.start())
							.with_message("Missing Code Language")
							.with_label(
								Label::new((token.source().clone(), lang.range()))
									.with_message("No language specified")
									.with_color(state.parser.colors().error),
							)
							.finish(),
					);

					return reports;
				}
				if Code::get_syntaxes()
					.find_syntax_by_name(code_lang.as_str())
					.is_none()
				{
					reports.push(
						Report::build(ReportKind::Error, token.source(), lang.start())
							.with_message("Unknown Code Language")
							.with_label(
								Label::new((token.source().clone(), lang.range()))
									.with_message(format!(
										"Language `{}` cannot be found",
										code_lang.fg(state.parser.colors().info)
									))
									.with_color(state.parser.colors().error),
							)
							.finish(),
					);

					return reports;
				}

				code_lang
			}
		};

		let mut code_content = if index == 0 {
			util::process_escaped('\\', "```", matches.get(4).unwrap().as_str())
		} else {
			util::process_escaped('\\', "``", matches.get(3).unwrap().as_str())
		};
		if code_content.bytes().last() == Some('\n' as u8)
		// Remove newline
		{
			code_content.pop();
		}

		if code_content.is_empty() {
			reports.push(
				Report::build(ReportKind::Error, token.source(), token.start())
					.with_message("Missing code content")
					.with_label(
						Label::new((token.source().clone(), token.range.clone()))
							.with_message("Code content cannot be empty")
							.with_color(state.parser.colors().error),
					)
					.finish(),
			);
			return reports;
		}

		let theme = document
			.get_variable("code.theme")
			.and_then(|var| Some(var.to_string()));

		if index == 0
		// Block
		{
			let code_name = matches.get(3).and_then(|name| {
				let code_name = name.as_str().trim_end().trim_start().to_string();
				(!code_name.is_empty()).then_some(code_name)
			});
			let line_offset =
				match properties.get("line_offset", |prop, value| {
					value.parse::<usize>().map_err(|e| (prop, e))
				}) {
					Ok((_prop, offset)) => offset,
					Err(e) => {
						match e {
							PropertyMapError::ParseError((prop, err)) => {
								reports.push(
							Report::build(ReportKind::Error, token.source(), token.start())
							.with_message("Invalid Code Property")
							.with_label(
								Label::new((token.source().clone(), token.start()+1..token.end()))
								.with_message(format!("Property `line_offset: {}` cannot be converted: {}",
										prop.fg(state.parser.colors().info),
										err.fg(state.parser.colors().error)))
								.with_color(state.parser.colors().warning))
							.finish());
								return reports;
							}
							PropertyMapError::NotFoundError(err) => {
								reports.push(
									Report::build(ReportKind::Error, token.source(), token.start())
										.with_message("Invalid Code Property")
										.with_label(
											Label::new((
												token.source().clone(),
												token.start() + 1..token.end(),
											))
											.with_message(format!(
												"Property `{}` doesn't exist",
												err.fg(state.parser.colors().info)
											))
											.with_color(state.parser.colors().warning),
										)
										.finish(),
								);
								return reports;
							}
						}
					}
				};

			state.push(
				document,
				Box::new(Code::new(
					token.clone(),
					CodeKind::FullBlock,
					code_lang,
					code_name,
					code_content,
					theme,
					line_offset,
				)),
			);
		} else
		// Maybe inline
		{
			let block = if code_content.contains('\n') {
				CodeKind::MiniBlock
			} else {
				CodeKind::Inline
			};

			state.push(
				document,
				Box::new(Code::new(
					token.clone(),
					block,
					code_lang,
					None,
					code_content,
					theme,
					1,
				)),
			);
		}

		reports
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];
		bindings.push((
			"push_inline".to_string(),
			lua.create_function(|_, (language, content): (String, String)| {
				CTX.with_borrow(|ctx| {
					ctx.as_ref().map(|ctx| {
						let theme = ctx
							.document
							.get_variable("code.theme")
							.and_then(|var| Some(var.to_string()));

						ctx.state.push(
							ctx.document,
							Box::new(Code {
								location: ctx.location.clone(),
								block: CodeKind::Inline,
								language,
								name: None,
								code: content,
								theme,
								line_offset: 1,
							}),
						);
					})
				});

				Ok(())
			})
			.unwrap(),
		));

		bindings.push((
			"push_miniblock".to_string(),
			lua.create_function(
				|_, (language, content, line_offset): (String, String, Option<usize>)| {
					CTX.with_borrow(|ctx| {
						ctx.as_ref().map(|ctx| {
							let theme = ctx
								.document
								.get_variable("code.theme")
								.and_then(|var| Some(var.to_string()));

							ctx.state.push(
								ctx.document,
								Box::new(Code {
									location: ctx.location.clone(),
									block: CodeKind::MiniBlock,
									language,
									name: None,
									code: content,
									theme,
									line_offset: line_offset.unwrap_or(1),
								}),
							);
						})
					});

					Ok(())
				},
			)
			.unwrap(),
		));

		bindings.push((
			"push_block".to_string(),
			lua.create_function(
				|_,
				 (language, name, content, line_offset): (
					String,
					Option<String>,
					String,
					Option<usize>,
				)| {
					CTX.with_borrow(|ctx| {
						ctx.as_ref().map(|ctx| {
							let theme = ctx
								.document
								.get_variable("code.theme")
								.and_then(|var| Some(var.to_string()));

							ctx.state.push(
								ctx.document,
								Box::new(Code {
									location: ctx.location.clone(),
									block: CodeKind::FullBlock,
									language,
									name,
									code: content,
									theme,
									line_offset: line_offset.unwrap_or(1),
								}),
							);
						})
					});

					Ok(())
				},
			)
			.unwrap(),
		));

		bindings
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;

	#[test]
	fn code_block() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
```[line_offset=32] C, Some Code...
static int INT32_MIN = 0x80000000;
```
%<nml.code.push_block("Lua", "From Lua", "print(\"Hello, World!\")", nil)>%
``Rust,
fn fact(n: usize) -> usize
{
	match n
	{
		0 | 1 => 1,
		_ => n * fact(n-1)
	}
}
``
%<nml.code.push_miniblock("Bash", "NUM=$(($RANDOM % 10))", 18)>%
"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

		let borrow = doc.content().borrow();
		let found = borrow
			.iter()
			.filter_map(|e| e.downcast_ref::<Code>())
			.collect::<Vec<_>>();

		assert_eq!(found[0].block, CodeKind::FullBlock);
		assert_eq!(found[0].language, "C");
		assert_eq!(found[0].name, Some("Some Code...".to_string()));
		assert_eq!(found[0].code, "static int INT32_MIN = 0x80000000;");
		assert_eq!(found[0].line_offset, 32);

		assert_eq!(found[1].block, CodeKind::FullBlock);
		assert_eq!(found[1].language, "Lua");
		assert_eq!(found[1].name, Some("From Lua".to_string()));
		assert_eq!(found[1].code, "print(\"Hello, World!\")");
		assert_eq!(found[1].line_offset, 1);

		assert_eq!(found[2].block, CodeKind::MiniBlock);
		assert_eq!(found[2].language, "Rust");
		assert_eq!(found[2].name, None);
		assert_eq!(found[2].code, "fn fact(n: usize) -> usize\n{\n\tmatch n\n\t{\n\t\t0 | 1 => 1,\n\t\t_ => n * fact(n-1)\n\t}\n}");
		assert_eq!(found[2].line_offset, 1);

		assert_eq!(found[3].block, CodeKind::MiniBlock);
		assert_eq!(found[3].language, "Bash");
		assert_eq!(found[3].name, None);
		assert_eq!(found[3].code, "NUM=$(($RANDOM % 10))");
		assert_eq!(found[3].line_offset, 18);
	}

	#[test]
	fn code_inline() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
``C, int fact(int n)``
``Plain Text, Text in a code block!``
%<nml.code.push_inline("C++", "std::vector<std::vector<int>> u;")>%
			"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

		let borrow = doc.content().borrow();
		let found = borrow
			.first()
			.unwrap()
			.as_container()
			.unwrap()
			.contained()
			.iter()
			.filter_map(|e| e.downcast_ref::<Code>())
			.collect::<Vec<_>>();

		assert_eq!(found[0].block, CodeKind::Inline);
		assert_eq!(found[0].language, "C");
		assert_eq!(found[0].name, None);
		assert_eq!(found[0].code, "int fact(int n)");
		assert_eq!(found[0].line_offset, 1);

		assert_eq!(found[1].block, CodeKind::Inline);
		assert_eq!(found[1].language, "Plain Text");
		assert_eq!(found[1].name, None);
		assert_eq!(found[1].code, "Text in a code block!");
		assert_eq!(found[1].line_offset, 1);

		assert_eq!(found[2].block, CodeKind::Inline);
		assert_eq!(found[2].language, "C++");
		assert_eq!(found[2].name, None);
		assert_eq!(found[2].code, "std::vector<std::vector<int>> u;");
		assert_eq!(found[2].line_offset, 1);
	}
}
