use std::collections::HashMap;
use std::sync::Once;

use ariadne::Fmt;
use crypto::digest::Digest;
use crypto::sha2::Sha512;
use mlua::Function;
use mlua::Lua;
use parser::util::escape_source;
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
use crate::lsp::semantic::Semantics;
use crate::lua::kernel::CTX;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;
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

			result += "<div class=\"code-block-content\"><table cellspacing=\"0\">"
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
			result += "<div class=\"code-block\"><div class=\"code-block-content\"><table cellspacing=\"0\">";

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

	fn compile(
		&self,
		compiler: &Compiler,
		_document: &dyn Document,
		_cursor: usize,
	) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				static CACHE_INIT: Once = Once::new();
				CACHE_INIT.call_once(|| {
					if let Some(con) = compiler.cache() {
						if let Err(e) = Code::init(con) {
							eprintln!("Unable to create cache table: {e}");
						}
					}
				});

				if let Some(con) = compiler.cache() {
					match self.cached(con, |s| s.highlight_html(compiler)) {
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
			Property::new("Line number offset".to_string(), Some("1".to_string())),
		);
		Self {
			re: [
				Regex::new(
					r"(?:^|\n)```(?:\[((?:\\.|[^\\\\])*?)\])?(.*?)(?:,(.*))?\n((?:\\(?:.|\n)|[^\\\\])*?)```",
				)
				.unwrap(),
				Regex::new(
					r"``(?:\[((?:\\.|[^\\\\])*?)\])?(?:([^\r\n`]*?)(?:,|\n))?((?:\\(?:.|\n)|[^\\\\])*?)``",
				)
				.unwrap(),
			],
			properties: PropertyParser { properties: props },
		}
	}
}

impl RegexRule for CodeRule {
	fn name(&self) -> &'static str { "Code" }

	fn previous(&self) -> Option<&'static str> { Some("Block") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn enabled(&self, mode: &ParseMode, id: usize) -> bool { !mode.paragraph_only || id != 0 }

	fn on_regex_match(
		&self,
		index: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		// Properties
		let prop_source = escape_source(
			token.source(),
			matches.get(1).map_or(0..0, |m| m.range()),
			"Code Properties".into(),
			'\\',
			"]",
		);
		let properties =
			match self
				.properties
				.parse("Code", &mut reports, state, prop_source.into())
			{
				Some(props) => props,
				None => return reports,
			};

		let code_lang = match matches.get(2) {
			None => "Plain Text".to_string(),
			Some(lang) => {
				let mut code_lang = lang.as_str().trim_start().trim_end().to_string();
				if code_lang.is_empty() {
					code_lang = "Plain Text".into();
				}
				if Code::get_syntaxes()
					.find_syntax_by_name(code_lang.as_str())
					.is_none()
				{
					report_err!(
						&mut reports,
						token.source(),
						"Unknown Code Language".into(),
						span(
							lang.range(),
							format!(
								"Language `{}` cannot be found",
								code_lang.fg(state.parser.colors().info)
							)
						)
					);

					return reports;
				}

				code_lang
			}
		};

		let mut code_content = if index == 0 {
			util::escape_text('\\', "```", matches.get(4).unwrap().as_str(), false)
		} else {
			util::escape_text(
				'\\',
				"``",
				matches.get(3).unwrap().as_str(),
				!matches.get(3).unwrap().as_str().contains('\n'),
			)
		};
		if code_content.bytes().last() == Some(b'\n')
		// Remove newline
		{
			code_content.pop();
		}

		if code_content.is_empty() {
			report_err!(
				&mut reports,
				token.source(),
				"Empty Code Content".into(),
				span(token.range.clone(), "Code content cannot be empty".into())
			);
			return reports;
		}

		let theme = document
			.get_variable("code.theme")
			.map(|var| var.to_string());

		if index == 0
		// Block
		{
			let code_name = matches.get(3).and_then(|name| {
				let code_name = name.as_str().trim_end().trim_start().to_string();
				(!code_name.is_empty()).then_some(code_name)
			});
			let line_offset = match properties.get(&mut reports, "line_offset", |_, value| {
				value.value.parse::<usize>()
			}) {
				Some(line_offset) => line_offset,
				_ => return reports,
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

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			let range = matches
				.get(0)
				.map(|m| {
					if token.source().content().as_bytes()[m.start()] == b'\n' {
						m.start() + 1..m.end()
					} else {
						m.range()
					}
				})
				.unwrap();
			sems.add(
				range.start..range.start + if index == 0 { 3 } else { 2 },
				tokens.code_sep,
			);
			if let Some(props) = matches.get(1).map(|m| m.range()) {
				sems.add(props.start - 1..props.start, tokens.code_props_sep);
				sems.add(props.end..props.end + 1, tokens.code_props_sep);
			}
			if let Some(lang) = matches.get(2).map(|m| m.range()) {
				sems.add(lang.clone(), tokens.code_lang);
			}
			if index == 0 {
				if let Some(title) = matches.get(3).map(|m| m.range()) {
					sems.add(title.clone(), tokens.code_title);
				}
				sems.add(matches.get(4).unwrap().range(), tokens.code_content);
			} else {
				sems.add(matches.get(3).unwrap().range(), tokens.code_content);
			}
			sems.add(
				range.end - if index == 0 { 3 } else { 2 }..range.end,
				tokens.code_sep,
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
							.map(|var| var.to_string());

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
								.map(|var| var.to_string());

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
								.map(|var| var.to_string());

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
	use crate::validate_semantics;
	use std::rc::Rc;

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
		let (doc, _) = parser.parse(
			ParserState::new(&parser, None),
			source,
			None,
			ParseMode::default(),
		);

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
		assert_eq!(found[2].code, "\nfn fact(n: usize) -> usize\n{\n\tmatch n\n\t{\n\t\t0 | 1 => 1,\n\t\t_ => n * fact(n-1)\n\t}\n}");
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
		let (doc, _) = parser.parse(
			ParserState::new(&parser, None),
			source,
			None,
			ParseMode::default(),
		);

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

	#[test]
	fn semantic() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
```[line_offset=15] C, Title
test code
```
``C, Single Line``
		"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (_, state) = parser.parse(
			ParserState::new_with_semantics(&parser, None),
			source.clone(),
			None,
			ParseMode::default(),
		);
		validate_semantics!(state, source.clone(), 0,
			code_sep { delta_line == 1, delta_start == 0, length == 3 };
			code_props_sep { delta_line == 0, delta_start == 3, length == 1 };
			prop_name { delta_line == 0, delta_start == 1, length == 11 };
			prop_equal { delta_line == 0, delta_start == 11, length == 1 };
			prop_value { delta_line == 0, delta_start == 1, length == 2 };
			code_props_sep { delta_line == 0, delta_start == 2, length == 1 };
			code_lang { delta_line == 0, delta_start == 1, length == 2 };
			code_title { delta_line == 0, delta_start == 3, length == 6 };
			code_content { delta_line == 1, delta_start == 0, length == 10 };
			code_sep { delta_line == 1, delta_start == 0, length == 3 };

			code_sep { delta_line == 1, delta_start == 0, length == 2 };
			code_lang { delta_line == 0, delta_start == 2, length == 1 };
			code_content { delta_line == 0, delta_start == 2, length == 12 };
			code_sep { delta_line == 0, delta_start == 12, length == 2 };
		);
	}
}
