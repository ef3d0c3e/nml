use std::collections::HashMap;
use std::io::Read;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Once;

use ariadne::Fmt;
use crypto::digest::Digest;
use crypto::sha2::Sha512;
use lsp::code::CodeRange;
use mlua::Function;
use mlua::Lua;
use parser::util::escape_source;
use regex::Captures;
use regex::Regex;

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
use crate::parser::util;

#[derive(Debug, PartialEq, Eq)]
enum TexKind {
	Block,
	Inline,
}

impl FromStr for TexKind {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"inline" => Ok(TexKind::Inline),
			"block" => Ok(TexKind::Block),
			_ => Err(format!("Unknown kind: {s}")),
		}
	}
}

impl From<&TexKind> for ElemKind {
	fn from(value: &TexKind) -> Self {
		match value {
			TexKind::Inline => ElemKind::Inline,
			_ => ElemKind::Block,
		}
	}
}

#[derive(Debug)]
struct Tex {
	pub(self) location: Token,
	pub(self) mathmode: bool,
	pub(self) kind: TexKind,
	pub(self) env: String,
	pub(self) tex: String,
	pub(self) caption: Option<String>,
}

impl Tex {
	fn format_latex(fontsize: &String, preamble: &String, tex: &String) -> FormattedTex {
		FormattedTex(format!(
			r"\documentclass[{}pt,preview]{{standalone}}
{}
\begin{{document}}
\begin{{preview}}
{}
\end{{preview}}
\end{{document}}",
			fontsize, preamble, tex
		))
	}
}

struct FormattedTex(String);

impl FormattedTex {
	/// Renders latex to svg
	fn latex_to_svg(&self, exec: &String, fontsize: &String) -> Result<String, String> {
		print!("Rendering LaTex `{}`... ", self.0);
		let process = match Command::new(exec)
			.arg("--fontsize")
			.arg(fontsize)
			.stdout(Stdio::piped())
			.stdin(Stdio::piped())
			.spawn()
		{
			Err(e) => return Err(format!("Could not spawn `{exec}`: {}", e)),
			Ok(process) => process,
		};

		if let Err(e) = process.stdin.unwrap().write_all(self.0.as_bytes()) {
			panic!("Unable to write to `latex2svg`'s stdin: {}", e);
		}

		let mut result = String::new();
		if let Err(e) = process.stdout.unwrap().read_to_string(&mut result) {
			panic!("Unable to read `latex2svg` stdout: {}", e)
		}
		println!("Done!");

		Ok(result)
	}
}

impl Cached for FormattedTex {
	type Key = String;
	type Value = String;

	fn sql_table() -> &'static str {
		"CREATE TABLE IF NOT EXISTS cached_tex (
				digest TEXT PRIMARY KEY,
				svg    BLOB NOT NULL);"
	}

	fn sql_get_query() -> &'static str { "SELECT svg FROM cached_tex WHERE digest = (?1)" }

	fn sql_insert_query() -> &'static str { "INSERT INTO cached_tex (digest, svg) VALUES (?1, ?2)" }

	fn key(&self) -> <Self as Cached>::Key {
		let mut hasher = Sha512::new();
		hasher.input(self.0.as_bytes());

		hasher.result_str()
	}
}

impl Element for Tex {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { (&self.kind).into() }

	fn element_name(&self) -> &'static str { "LaTeX" }

	fn compile(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		_cursor: usize,
	) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				static CACHE_INIT: Once = Once::new();
				CACHE_INIT.call_once(|| {
					if let Some(con) = compiler.cache() {
						if let Err(e) = FormattedTex::init(con) {
							eprintln!("Unable to create cache table: {e}");
						}
					}
				});

				let exec = document
					.get_variable(format!("tex.{}.exec", self.env).as_str())
					.map_or("latex2svg".to_string(), |var| var.to_string());
				// FIXME: Because fontsize is passed as an arg, verify that it cannot be used to execute python/shell code
				let fontsize = document
					.get_variable(format!("tex.{}.fontsize", self.env).as_str())
					.map_or("12".to_string(), |var| var.to_string());
				let preamble = document
					.get_variable(format!("tex.{}.preamble", self.env).as_str())
					.map_or("".to_string(), |var| var.to_string());
				let prepend = if self.mathmode {
					"".to_string()
				} else {
					document
						.get_variable(format!("tex.{}.block_prepend", self.env).as_str())
						.map_or("".to_string(), |var| var.to_string() + "\n")
				};

				let latex = if self.mathmode {
					Tex::format_latex(&fontsize, &preamble, &format!("${{{}}}$", self.tex))
				} else {
					Tex::format_latex(&fontsize, &preamble, &format!("{prepend}{}", self.tex))
				};

				let result = if let Some(con) = compiler.cache() {
					match latex.cached(con, |s| s.latex_to_svg(&exec, &fontsize)) {
						Ok(s) => Ok(s),
						Err(e) => match e {
							CachedError::SqlErr(e) => {
								Err(format!("Querying the cache failed: {e}"))
							}
							CachedError::GenErr(e) => Err(e),
						},
					}
				} else {
					latex.latex_to_svg(&exec, &fontsize)
				};

				// Caption
				result.map(|mut result| {
					if let (Some(caption), Some(start)) = (&self.caption, result.find('>')) {
						result.insert_str(
							start + 1,
							format!(
								"<title>{}</title>",
								Compiler::sanitize(Target::HTML, caption)
							)
							.as_str(),
						);
					}
					result
				})
			}
			_ => todo!("Unimplemented"),
		}
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::tex")]
pub struct TexRule {
	re: [Regex; 2],
	properties: PropertyParser,
}

impl TexRule {
	pub fn new() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"env".to_string(),
			Property::new("Tex environment".to_string(), Some("main".to_string())),
		);
		props.insert(
			"kind".to_string(),
			Property::new("Element display kind".to_string(), None),
		);
		props.insert(
			"caption".to_string(),
			Property::new("Latex caption".to_string(), None),
		);
		Self {
			re: [
				Regex::new(r"\$\|(?:\[((?:\\.|[^\\\\])*?)\])?(?:((?:\\.|[^\\\\])*?)\|\$)?")
					.unwrap(),
				Regex::new(r"\$(?:\[((?:\\.|[^\\\\])*?)\])?(?:((?:\\.|[^\\\\])*?)\$)?").unwrap(),
			],
			properties: PropertyParser { properties: props },
		}
	}
}

impl RegexRule for TexRule {
	fn name(&self) -> &'static str { "Tex" }

	fn previous(&self) -> Option<&'static str> { Some("Code") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn enabled(&self, _mode: &ParseMode, _id: usize) -> bool { true }

	fn on_regex_match(
		&self,
		index: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		let tex_content = match matches.get(2) {
			// Unterminated `$`
			None => {
				report_err!(
					&mut reports,
					token.source(),
					"Unterminated Tex Code".into(),
					span(
						token.range.clone(),
						format!(
							"Missing terminating `{}` after first `{}`",
							["|$", "$"][index].fg(state.parser.colors().info),
							["$|", "$"][index].fg(state.parser.colors().info)
						)
					)
				);
				return reports;
			}
			Some(content) => {
				let processed = util::escape_text(
					'\\',
					["|$", "$"][index],
					content.as_str().trim_start().trim_end(),
					true,
				);

				if processed.is_empty() {
					report_err!(
						&mut reports,
						token.source(),
						"Empty Tex Code".into(),
						span(content.range(), "Tex code is empty".into())
					);
				}
				processed
			}
		};

		// Properties
		let prop_source = escape_source(
			token.source(),
			matches.get(1).map_or(0..0, |m| m.range()),
			"Tex Properties".into(),
			'\\',
			"]",
		);
		let properties = match self.properties.parse(
			"Raw Code",
			&mut reports,
			state,
			Token::new(0..prop_source.content().len(), prop_source),
		) {
			Some(props) => props,
			None => return reports,
		};

		let (tex_kind, caption, tex_env) = match (
			properties.get_or(
				&mut reports,
				"kind",
				if index == 1 {
					TexKind::Inline
				} else {
					TexKind::Block
				},
				|_, value| TexKind::from_str(value.value.as_str()),
			),
			properties.get_opt(&mut reports, "caption", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}),
			properties.get(&mut reports, "env", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}),
		) {
			(Some(tex_kind), Some(caption), Some(tex_env)) => (tex_kind, caption, tex_env),
			_ => return reports,
		};

		// Code ranges
		if let Some(coderanges) = CodeRange::from_source(token.source(), &state.shared.lsp) {
			if index == 0 && tex_content.contains('\n')
			{
				let range = matches
					.get(2)
					.map(|m| {
						if token.source().content().as_bytes()[m.start()] == b'\n' {
							m.start() + 1..m.end()
						} else {
							m.range()
						}
					})
				.unwrap();

				coderanges.add(range, "Latex".into());
			}
		}

		state.push(
			document,
			Box::new(Tex {
				mathmode: index == 1,
				location: token.clone(),
				kind: tex_kind,
				env: tex_env,
				tex: tex_content,
				caption,
			}),
		);

		// Semantics
		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			let range = token.range;
			sems.add(
				range.start..range.start + if index == 0 { 2 } else { 1 },
				tokens.tex_sep,
			);
			if let Some(props) = matches.get(1).map(|m| m.range()) {
				sems.add(props.start - 1..props.start, tokens.tex_props_sep);
				sems.add(props.end..props.end + 1, tokens.tex_props_sep);
			}
			sems.add(matches.get(2).unwrap().range(), tokens.tex_content);
			sems.add(
				range.end - if index == 0 { 2 } else { 1 }..range.end,
				tokens.tex_sep,
			);
		}

		reports
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];
		bindings.push((
			"push_math".to_string(),
			lua.create_function(
				|_, (kind, tex, env, caption): (String, String, Option<String>, Option<String>)| {
					let mut result = Ok(());
					CTX.with_borrow(|ctx| {
						ctx.as_ref().map(|ctx| {
							let kind = match TexKind::from_str(kind.as_str()) {
								Ok(kind) => kind,
								Err(err) => {
									result = Err(mlua::Error::BadArgument {
										to: Some("push".to_string()),
										pos: 2,
										name: Some("kind".to_string()),
										cause: Arc::new(mlua::Error::external(format!(
											"Unable to get tex kind: {err}"
										))),
									});
									return;
								}
							};

							ctx.state.push(
								ctx.document,
								Box::new(Tex {
									location: ctx.location.clone(),
									mathmode: true,
									kind,
									env: env.unwrap_or("main".to_string()),
									tex,
									caption,
								}),
							);
						})
					});

					result
				},
			)
			.unwrap(),
		));

		bindings.push((
			"push".to_string(),
			lua.create_function(
				|_, (kind, tex, env, caption): (String, String, Option<String>, Option<String>)| {
					let mut result = Ok(());
					CTX.with_borrow(|ctx| {
						ctx.as_ref().map(|ctx| {
							let kind = match TexKind::from_str(kind.as_str()) {
								Ok(kind) => kind,
								Err(err) => {
									result = Err(mlua::Error::BadArgument {
										to: Some("push".to_string()),
										pos: 2,
										name: Some("kind".to_string()),
										cause: Arc::new(mlua::Error::external(format!(
											"Unable to get tex kind: {err}"
										))),
									});
									return;
								}
							};

							ctx.state.push(
								ctx.document,
								Box::new(Tex {
									location: ctx.location.clone(),
									mathmode: false,
									kind,
									env: env.unwrap_or("main".to_string()),
									tex,
									caption,
								}),
							);
						})
					});

					result
				},
			)
			.unwrap(),
		));

		bindings
	}
}

#[cfg(test)]
mod tests {
	use crate::elements::paragraph::Paragraph;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;
	use crate::validate_semantics;
	use std::rc::Rc;

	use super::*;

	#[test]
	fn tex_block() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
$[kind=block, caption=Some\, text\\] 1+1=2	$
$|[env=another] Non Math \LaTeX |$
$[kind=block,env=another] e^{i\pi}=-1$
%<nml.tex.push_math("block", "1+1=2", nil, "Some, text\\")>%
%<nml.tex.push("block", "Non Math \\LaTeX", "another", nil)>%
%<nml.tex.push_math("block", "e^{i\\pi}=-1", "another", nil)>%
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

		validate_document!(doc.content().borrow(), 0,
			Tex { mathmode == true, tex == "1+1=2", env == "main", caption == Some("Some, text\\\\".to_string()) };
			Tex { mathmode == false, tex == "Non Math \\LaTeX", env == "another" };
			Tex { mathmode == true, tex == "e^{i\\pi}=-1", env == "another" };
			Tex { mathmode == true, tex == "1+1=2", env == "main", caption == Some("Some, text\\".to_string()) };
			Tex { mathmode == false, tex == "Non Math \\LaTeX", env == "another" };
			Tex { mathmode == true, tex == "e^{i\\pi}=-1", env == "another" };
		);
	}

	#[test]
	fn tex_inline() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
$[ caption=Some\, text\\] 1+1=2	$
$|[env=another, kind=inline  ,   caption = Enclosed \].  ] Non Math \LaTeX|$
$[env=another] e^{i\pi}=-1$
%<nml.tex.push_math("inline", "1+1=2", "main", "Some, text\\")>%
%<nml.tex.push("inline", "Non Math \\LaTeX", "another", "Enclosed ].")>%
%<nml.tex.push_math("inline", "e^{i\\pi}=-1", "another", nil)>%
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

		validate_document!(doc.content().borrow(), 0,
			Paragraph {
				Tex { mathmode == true, tex == "1+1=2", env == "main", caption == Some("Some, text\\\\".to_string()) };
				Tex { mathmode == false, tex == "Non Math \\LaTeX", env == "another", caption == Some("Enclosed ].".to_string()) };
				Tex { mathmode == true, tex == "e^{i\\pi}=-1", env == "another" };
				Tex { mathmode == true, tex == "1+1=2", env == "main", caption == Some("Some, text\\".to_string()) };
				Tex { mathmode == false, tex == "Non Math \\LaTeX", env == "another", caption == Some("Enclosed ].".to_string()) };
				Tex { mathmode == true, tex == "e^{i\\pi}=-1", env == "another" };
			};
		);
	}

	#[test]
	fn semantic() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
$[kind=inline]\LaTeX$
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
			tex_sep { delta_line == 1, delta_start == 0, length == 1 };
			tex_props_sep { delta_line == 0, delta_start == 1, length == 1 };
			prop_name { delta_line == 0, delta_start == 1, length == 4 };
			prop_equal { delta_line == 0, delta_start == 4, length == 1 };
			prop_value { delta_line == 0, delta_start == 1, length == 6 };
			tex_props_sep { delta_line == 0, delta_start == 6, length == 1 };
			tex_content { delta_line == 0, delta_start == 1, length == 6 };
			tex_sep { delta_line == 0, delta_start == 6, length == 1 };
		);
	}
}
