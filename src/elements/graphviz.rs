use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Once;

use crate::lua::kernel::CTX;
use crate::parser::parser::ParserState;
use crate::parser::util::Property;
use crate::parser::util::PropertyMapError;
use crate::parser::util::PropertyParser;
use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use crypto::digest::Digest;
use crypto::sha2::Sha512;
use graphviz_rust::cmd::Format;
use graphviz_rust::cmd::Layout;
use graphviz_rust::exec_dot;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use regex::Captures;
use regex::Regex;

use crate::cache::cache::Cached;
use crate::cache::cache::CachedError;
use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::util;

#[derive(Debug)]
struct Graphviz {
	pub location: Token,
	pub dot: String,
	pub layout: Layout,
	pub width: String,
}

fn layout_from_str(value: &str) -> Result<Layout, String> {
	match value {
		"dot" => Ok(Layout::Dot),
		"neato" => Ok(Layout::Neato),
		"fdp" => Ok(Layout::Fdp),
		"sfdp" => Ok(Layout::Sfdp),
		"circo" => Ok(Layout::Circo),
		"twopi" => Ok(Layout::Twopi),
		"osage" => Ok(Layout::Asage), // typo  in graphviz_rust ?
		"patchwork" => Ok(Layout::Patchwork),
		_ => Err(format!("Unknown layout: {value}")),
	}
}

impl Graphviz {
	/// Renders dot to svg
	fn dot_to_svg(&self) -> Result<String, String> {
		print!("Rendering Graphviz `{}`... ", self.dot);

		let svg = match exec_dot(
			self.dot.clone(),
			vec![self.layout.into(), Format::Svg.into()],
		) {
			Ok(svg) => {
				let out = String::from_utf8_lossy(svg.as_slice());
				let svg_start = out.find("<svg").unwrap(); // Remove svg header
				let split_at = out.split_at(svg_start).1.find('\n').unwrap();

				let mut result = format!("<svg width=\"{}\"", self.width);
				result.push_str(out.split_at(svg_start + split_at).1);

				result
			}
			Err(e) => return Err(format!("Unable to execute dot: {e}")),
		};
		println!("Done!");

		Ok(svg)
	}
}

impl Cached for Graphviz {
	type Key = String;
	type Value = String;

	fn sql_table() -> &'static str {
		"CREATE TABLE IF NOT EXISTS cached_dot (
				digest TEXT PRIMARY KEY,
				svg    BLOB NOT NULL);"
	}

	fn sql_get_query() -> &'static str { "SELECT svg FROM cached_dot WHERE digest = (?1)" }

	fn sql_insert_query() -> &'static str { "INSERT INTO cached_dot (digest, svg) VALUES (?1, ?2)" }

	fn key(&self) -> <Self as Cached>::Key {
		let mut hasher = Sha512::new();
		hasher.input((self.layout as usize).to_be_bytes().as_slice());
		hasher.input(self.width.as_bytes());
		hasher.input(self.dot.as_bytes());

		hasher.result_str()
	}
}

impl Element for Graphviz {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Block }

	fn element_name(&self) -> &'static str { "Graphviz" }

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
						if let Err(e) = Graphviz::init(con) {
							eprintln!("Unable to create cache table: {e}");
						}
					}
				});
				// TODO: Format svg in a div

				if let Some(con) = compiler.cache() {
					match self.cached(con, |s| s.dot_to_svg()) {
						Ok(s) => Ok(s),
						Err(e) => match e {
							CachedError::SqlErr(e) => {
								Err(format!("Querying the cache failed: {e}"))
							}
							CachedError::GenErr(e) => Err(e),
						},
					}
				} else {
					match self.dot_to_svg() {
						Ok(svg) => Ok(svg),
						Err(e) => Err(e),
					}
				}
			}
			_ => todo!("Unimplemented"),
		}
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::graphviz")]
pub struct GraphRule {
	re: [Regex; 1],
	properties: PropertyParser,
}

impl GraphRule {
	pub fn new() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"layout".to_string(),
			Property::new(
				true,
				"Graphviz layout engine see <https://graphviz.org/docs/layouts/>".to_string(),
				Some("dot".to_string()),
			),
		);
		props.insert(
			"width".to_string(),
			Property::new(true, "SVG width".to_string(), Some("100%".to_string())),
		);
		Self {
			re: [Regex::new(
				r"\[graph\](?:\[((?:\\.|[^\[\]\\])*?)\])?(?:((?:\\.|[^\\\\])*?)\[/graph\])?",
			)
			.unwrap()],
			properties: PropertyParser { properties: props },
		}
	}
}

impl RegexRule for GraphRule {
	fn name(&self) -> &'static str { "Graphviz" }
	fn previous(&self) -> Option<&'static str> { Some("Tex") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn on_regex_match(
		&self,
		_: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let graph_content = match matches.get(2) {
			// Unterminated `[graph]`
			None => {
				reports.push(
					Report::build(ReportKind::Error, token.source(), token.start())
						.with_message("Unterminated Graph Code")
						.with_label(
							Label::new((token.source().clone(), token.range.clone()))
								.with_message(format!(
									"Missing terminating `{}` after first `{}`",
									"[/graph]".fg(state.parser.colors().info),
									"[graph]".fg(state.parser.colors().info)
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
					"[/graph]",
					content.as_str().trim_start().trim_end(),
				);

				if processed.is_empty() {
					reports.push(
						Report::build(ReportKind::Error, token.source(), content.start())
							.with_message("Empty Graph Code")
							.with_label(
								Label::new((token.source().clone(), content.range()))
									.with_message("Graph code is empty")
									.with_color(state.parser.colors().error),
							)
							.finish(),
					);
					return reports;
				}
				processed
			}
		};

		// Properties
		let properties = match matches.get(1) {
			None => match self.properties.default() {
				Ok(properties) => properties,
				Err(e) => {
					reports.push(
						Report::build(ReportKind::Error, token.source(), token.start())
							.with_message("Invalid Graph")
							.with_label(
								Label::new((token.source().clone(), token.range.clone()))
									.with_message(format!("Graph is missing property: {e}"))
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
								.with_message("Invalid Graph Properties")
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

		// Property "layout"
		let graph_layout = match properties.get("layout", |prop, value| {
			layout_from_str(value.as_str()).map_err(|e| (prop, e))
		}) {
			Ok((_prop, kind)) => kind,
			Err(e) => match e {
				PropertyMapError::ParseError((prop, err)) => {
					reports.push(
						Report::build(ReportKind::Error, token.source(), token.start())
							.with_message("Invalid Graph Property")
							.with_label(
								Label::new((token.source().clone(), token.range.clone()))
									.with_message(format!(
										"Property `layout: {}` cannot be converted: {}",
										prop.fg(state.parser.colors().info),
										err.fg(state.parser.colors().error)
									))
									.with_color(state.parser.colors().warning),
							)
							.finish(),
					);
					return reports;
				}
				PropertyMapError::NotFoundError(err) => {
					reports.push(
						Report::build(ReportKind::Error, token.source(), token.start())
							.with_message("Invalid Graph Property")
							.with_label(
								Label::new((
									token.source().clone(),
									token.start() + 1..token.end(),
								))
								.with_message(err)
								.with_color(state.parser.colors().warning),
							)
							.finish(),
					);
					return reports;
				}
			},
		};

		// FIXME: You can escape html, make sure we escape single "
		// Property "width"
		let graph_width = match properties.get("width", |_, value| -> Result<String, ()> {
			Ok(value.clone())
		}) {
			Ok((_, kind)) => kind,
			Err(e) => match e {
				PropertyMapError::NotFoundError(err) => {
					reports.push(
						Report::build(ReportKind::Error, token.source(), token.start())
							.with_message("Invalid Graph Property")
							.with_label(
								Label::new((
									token.source().clone(),
									token.start() + 1..token.end(),
								))
								.with_message(format!(
									"Property `{}` is missing",
									err.fg(state.parser.colors().info)
								))
								.with_color(state.parser.colors().warning),
							)
							.finish(),
					);
					return reports;
				}
				_ => panic!("Unknown error"),
			},
		};

		state.push(
			document,
			Box::new(Graphviz {
				location: token,
				dot: graph_content,
				layout: graph_layout,
				width: graph_width,
			}),
		);

		reports
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];
		bindings.push((
			"push".to_string(),
			lua.create_function(|_, (layout, width, dot): (String, String, String)| {
				let mut result = Ok(());

				CTX.with_borrow(|ctx| {
					ctx.as_ref().map(|ctx| {
						let layout = match layout_from_str(layout.as_str()) {
							Err(err) => {
								result = Err(BadArgument {
									to: Some("push".to_string()),
									pos: 1,
									name: Some("layout".to_string()),
									cause: Arc::new(mlua::Error::external(format!(
										"Unable to get layout type: {err}"
									))),
								});
								return;
							}
							Ok(layout) => layout,
						};

						ctx.state.push(
							ctx.document,
							Box::new(Graphviz {
								location: ctx.location.clone(),
								dot,
								layout,
								width,
							}),
						);
					})
				});

				result
			})
			.unwrap(),
		));

		bindings
	}
}

#[cfg(test)]
mod tests {
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;

	use super::*;

	#[test]
	pub fn parse() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
[graph][width=200px, layout=neato]
Some graph...
[/graph]
[graph]
Another graph
[/graph]
			"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

		validate_document!(doc.content().borrow(), 0,
			Graphviz { width == "200px", dot == "Some graph..." };
			Graphviz { dot == "Another graph" };
		);
	}

	#[test]
	pub fn lua() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
%<nml.graphviz.push("neato", "200px", "Some graph...")>%
%<nml.graphviz.push("dot", "", "Another graph")>%
			"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

		validate_document!(doc.content().borrow(), 0,
			Graphviz { width == "200px", dot == "Some graph..." };
			Graphviz { dot == "Another graph" };
		);
	}
}
