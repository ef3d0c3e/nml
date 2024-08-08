use crate::compiler::compiler::Compiler;
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
use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use regex::Captures;
use regex::Regex;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug)]
pub struct Raw {
	pub location: Token,
	pub kind: ElemKind,
	pub content: String,
}

impl Element for Raw {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { self.kind.clone() }

	fn element_name(&self) -> &'static str { "Raw" }

	fn compile(&self, _compiler: &Compiler, _document: &dyn Document) -> Result<String, String> {
		Ok(self.content.clone())
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::raw")]
pub struct RawRule {
	re: [Regex; 1],
	properties: PropertyParser,
}

impl RawRule {
	pub fn new() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"kind".to_string(),
			Property::new(
				true,
				"Element display kind".to_string(),
				Some("inline".to_string()),
			),
		);
		Self {
			re: [
				Regex::new(r"\{\?(?:\[((?:\\.|[^\[\]\\])*?)\])?(?:((?:\\.|[^\\\\])*?)(\?\}))?")
					.unwrap(),
			],
			properties: PropertyParser { properties: props },
		}
	}
}

impl RegexRule for RawRule {
	fn name(&self) -> &'static str { "Raw" }
	fn previous(&self) -> Option<&'static str> { Some("Variable Substitution") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn on_regex_match(
		&self,
		_index: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let raw_content = match matches.get(2) {
			// Unterminated
			None => {
				reports.push(
					Report::build(ReportKind::Error, token.source(), token.start())
						.with_message("Unterminated Raw Code")
						.with_label(
							Label::new((token.source().clone(), token.range.clone()))
								.with_message(format!(
									"Missing terminating `{}` after first `{}`",
									"?}".fg(state.parser.colors().info),
									"{?".fg(state.parser.colors().info)
								))
								.with_color(state.parser.colors().error),
						)
						.finish(),
				);
				return reports;
			}
			Some(content) => {
				let processed =
					util::process_escaped('\\', "?}", content.as_str().trim_start().trim_end());

				if processed.is_empty() {
					reports.push(
						Report::build(ReportKind::Warning, token.source(), content.start())
							.with_message("Empty Raw Code")
							.with_label(
								Label::new((token.source().clone(), content.range()))
									.with_message("Raw code is empty")
									.with_color(state.parser.colors().warning),
							)
							.finish(),
					);
				}
				processed
			}
		};

		let properties = match matches.get(1) {
			None => match self.properties.default() {
				Ok(properties) => properties,
				Err(e) => {
					reports.push(
						Report::build(ReportKind::Error, token.source(), token.start())
							.with_message("Invalid Raw Code")
							.with_label(
								Label::new((token.source().clone(), token.range.clone()))
									.with_message(format!("Raw code is missing properties: {e}"))
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
								.with_message("Invalid Raw Code Properties")
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

		let raw_kind: ElemKind = match properties.get("kind", |prop, value| {
			ElemKind::from_str(value.as_str()).map_err(|e| (prop, e))
		}) {
			Ok((_prop, kind)) => kind,
			Err(e) => match e {
				PropertyMapError::ParseError((prop, err)) => {
					reports.push(
						Report::build(ReportKind::Error, token.source(), token.start())
							.with_message("Invalid Raw Code Property")
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
									"Property `{}` is missing",
									err.fg(state.parser.colors().info)
								))
								.with_color(state.parser.colors().warning),
							)
							.finish(),
					);
					return reports;
				}
			},
		};

		state.push(
			document,
			Box::new(Raw {
				location: token.clone(),
				kind: raw_kind,
				content: raw_content,
			}),
		);

		reports
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];

		bindings.push((
			"push".to_string(),
			lua.create_function(|_, (kind, content): (String, String)| {
				// Validate kind
				let kind = match ElemKind::from_str(kind.as_str()) {
					Ok(kind) => kind,
					Err(e) => {
						return Err(BadArgument {
							to: Some("push".to_string()),
							pos: 1,
							name: Some("kind".to_string()),
							cause: Arc::new(mlua::Error::external(format!(
								"Wrong section kind specified: {e}"
							))),
						})
					}
				};

				CTX.with_borrow(|ctx| {
					ctx.as_ref().map(|ctx| {
						ctx.state.push(
							ctx.document,
							Box::new(Raw {
								location: ctx.location.clone(),
								kind,
								content,
							}),
						);
					})
				});

				Ok(())
			})
			.unwrap(),
		));

		bindings
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::elements::paragraph::Paragraph;
	use crate::elements::text::Text;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
use crate::parser::source::SourceFile;
	use crate::validate_document;

	#[test]
	fn parser() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
Break{?[kind=block] Raw?}NewParagraph{?<b>?}
				"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

		validate_document!(doc.content().borrow(), 0,
			Paragraph;
			Raw { kind == ElemKind::Block, content == "Raw" };
			Paragraph {
				Text;
				Raw { kind == ElemKind::Inline, content == "<b>" };
			};
		);
	}

	#[test]
	fn lua() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
Break%<nml.raw.push("block", "Raw")>%NewParagraph%<nml.raw.push("inline", "<b>")>%
				"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

		validate_document!(doc.content().borrow(), 0,
		Paragraph;
		Raw { kind == ElemKind::Block, content == "Raw" };
		Paragraph {
			Text;
			Raw { kind == ElemKind::Inline, content == "<b>" };
		};
		);
	}
}
