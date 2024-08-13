use std::collections::HashMap;
use std::io::Read;
use std::io::Write;
use std::ops::Range;
use std::process::Command;
use std::process::Stdio;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Once;

use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use crypto::digest::Digest;
use crypto::sha2::Sha512;
use regex::Captures;
use regex::Match;
use regex::Regex;

use crate::cache::cache::Cached;
use crate::cache::cache::CachedError;
use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::parser::ParserState;
use crate::parser::parser::ReportColors;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::util;
use crate::parser::util::Property;
use crate::parser::util::PropertyMap;
use crate::parser::util::PropertyMapError;
use crate::parser::util::PropertyParser;

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
		match process.stdout.unwrap().read_to_string(&mut result) {
			Err(e) => panic!("Unable to read `latex2svg` stdout: {}", e),
			Ok(_) => {}
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

	fn compile(&self, compiler: &Compiler, document: &dyn Document, _cursor: usize) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				static CACHE_INIT: Once = Once::new();
				CACHE_INIT.call_once(|| {
					if let Some(mut con) = compiler.cache() {
						if let Err(e) = FormattedTex::init(&mut con) {
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

				let result = if let Some(mut con) = compiler.cache() {
					match latex.cached(&mut con, |s| s.latex_to_svg(&exec, &fontsize)) {
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
			Property::new(
				true,
				"Tex environment".to_string(),
				Some("main".to_string()),
			),
		);
		props.insert(
			"kind".to_string(),
			Property::new(false, "Element display kind".to_string(), None),
		);
		props.insert(
			"caption".to_string(),
			Property::new(false, "Latex caption".to_string(), None),
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

	fn parse_properties(
		&self,
		colors: &ReportColors,
		token: &Token,
		m: &Option<Match>,
	) -> Result<PropertyMap, Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		match m {
			None => match self.properties.default() {
				Ok(properties) => Ok(properties),
				Err(e) => Err(
					Report::build(ReportKind::Error, token.source(), token.start())
						.with_message("Invalid Tex Properties")
						.with_label(
							Label::new((token.source().clone(), token.range.clone()))
								.with_message(format!("Tex is missing required property: {e}"))
								.with_color(colors.error),
						)
						.finish(),
				),
			},
			Some(props) => {
				let processed =
					util::process_escaped('\\', "]", props.as_str().trim_start().trim_end());
				match self.properties.parse(processed.as_str()) {
					Err(e) => Err(
						Report::build(ReportKind::Error, token.source(), props.start())
							.with_message("Invalid Tex Properties")
							.with_label(
								Label::new((token.source().clone(), props.range()))
									.with_message(e)
									.with_color(colors.error),
							)
							.finish(),
					),
					Ok(properties) => Ok(properties),
				}
			}
		}
	}
}

impl RegexRule for TexRule {
	fn name(&self) -> &'static str { "Tex" }
	fn previous(&self) -> Option<&'static str> { Some("Code") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn on_regex_match(
		&self,
		index: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let tex_content = match matches.get(2) {
			// Unterminated `$`
			None => {
				reports.push(
					Report::build(ReportKind::Error, token.source(), token.start())
						.with_message("Unterminated Tex Code")
						.with_label(
							Label::new((token.source().clone(), token.range.clone()))
								.with_message(format!(
									"Missing terminating `{}` after first `{}`",
									["|$", "$"][index].fg(state.parser.colors().info),
									["$|", "$"][index].fg(state.parser.colors().info)
								))
								.with_color(state.parser.colors().error),
						)
						.finish(),
				);
				return reports;
			}
			Some(content) => {
				let processed = util::process_escaped(
					'\\',
					["|$", "$"][index],
					content.as_str().trim_start().trim_end(),
				);

				if processed.is_empty() {
					reports.push(
						Report::build(ReportKind::Warning, token.source(), content.start())
							.with_message("Empty Tex Code")
							.with_label(
								Label::new((token.source().clone(), content.range()))
									.with_message("Tex code is empty")
									.with_color(state.parser.colors().warning),
							)
							.finish(),
					);
				}
				processed
			}
		};

		// Properties
		let properties = match self.parse_properties(state.parser.colors(), &token, &matches.get(1))
		{
			Ok(pm) => pm,
			Err(report) => {
				reports.push(report);
				return reports;
			}
		};

		// Tex kind
		let tex_kind = match properties.get("kind", |prop, value| {
			TexKind::from_str(value.as_str()).map_err(|e| (prop, e))
		}) {
			Ok((_prop, kind)) => kind,
			Err(e) => match e {
				PropertyMapError::ParseError((prop, err)) => {
					reports.push(
						Report::build(ReportKind::Error, token.source(), token.start())
							.with_message("Invalid Tex Property")
							.with_label(
								Label::new((token.source().clone(), token.range.clone()))
									.with_message(format!(
										"Property `kind: {}` cannot be converted: {}",
										prop.fg(state.parser.colors().info),
										err.fg(state.parser.colors().error)
									))
									.with_color(state.parser.colors().warning),
							)
							.finish(),
					);
					return reports;
				}
				PropertyMapError::NotFoundError(_) => {
					if index == 1 {
						TexKind::Inline
					} else {
						TexKind::Block
					}
				}
			},
		};

		// Caption
		let caption = properties
			.get("caption", |_, value| -> Result<String, ()> {
				Ok(value.clone())
			})
			.ok()
			.and_then(|(_, value)| Some(value));

		// Environ
		let tex_env = properties
			.get("env", |_, value| -> Result<String, ()> {
				Ok(value.clone())
			})
			.ok()
			.and_then(|(_, value)| Some(value))
			.unwrap();

		state.push(
			document,
			Box::new(Tex {
				mathmode: index == 1,
				location: token,
				kind: tex_kind,
				env: tex_env.to_string(),
				tex: tex_content,
				caption,
			}),
		);

		reports
	}
}

#[cfg(test)]
mod tests {
	use crate::elements::paragraph::Paragraph;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;

	use super::*;

	#[test]
	fn tex_block() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
$[kind=block, caption=Some\, text\\] 1+1=2	$
$|[env=another] Non Math \LaTeX |$
$[kind=block,env=another] e^{i\pi}=-1$
			"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

		validate_document!(doc.content().borrow(), 0,
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
			"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

		validate_document!(doc.content().borrow(), 0,
			Paragraph {
				Tex { mathmode == true, tex == "1+1=2", env == "main", caption == Some("Some, text\\".to_string()) };
				Tex { mathmode == false, tex == "Non Math \\LaTeX", env == "another" };
				Tex { mathmode == true, tex == "e^{i\\pi}=-1", env == "another" };
			};
		);
	}
}
