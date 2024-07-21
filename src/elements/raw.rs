use mlua::{Function, Lua};
use regex::{Captures, Regex};
use crate::{compiler::compiler::Compiler, document::element::{ElemKind, Element}, parser::{parser::Parser, rule::RegexRule, source::{Source, Token}, util::{self, Property, PropertyParser}}};
use ariadne::{Fmt, Label, Report, ReportKind};
use crate::document::document::Document;
use std::{collections::HashMap, ops::Range, rc::Rc, str::FromStr};

#[derive(Debug)]
struct Raw {
	location: Token,
	kind: ElemKind,
	content: String,
}

impl Raw {
    fn new(location: Token, kind: ElemKind, content: String) -> Self {
        Self { location, kind, content }
    }
}

impl Element for Raw {
    fn location(&self) -> &Token { &self.location }
    fn kind(&self) -> ElemKind { self.kind.clone() }

    fn element_name(&self) -> &'static str { "Raw" }

    fn to_string(&self) -> String { format!("{self:#?}") }

    fn compile(&self, compiler: &Compiler, _document: &Document) -> Result<String, String> {
		Ok(self.content.clone())
    }
}

pub struct RawRule {
	re: [Regex; 1],
	properties: PropertyParser,
}

impl RawRule {
	pub fn new() -> Self {
		let mut props = HashMap::new();
		props.insert("kind".to_string(),
			Property::new(
				true,
				"Element display kind".to_string(),
					Some("inline".to_string())));
		Self {
            re: [
				Regex::new(r"\{\?(?:\[((?:\\.|[^\[\]\\])*?)\])?(?:((?:\\.|[^\\\\])*?)(\?\}))?").unwrap()
			],
			properties: PropertyParser::new(props)
        }
	}
}

impl RegexRule for RawRule
{
    fn name(&self) -> &'static str { "Raw" }

    fn regexes(&self) -> &[regex::Regex] { &self.re }

    fn on_regex_match(&self, _index: usize, parser: &dyn Parser, document: &Document, token: Token, matches: Captures)
		-> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let raw_content = match matches.get(2)
		{
			// Unterminated
			None => {
				reports.push(
					Report::build(ReportKind::Error, token.source(), token.start())
					.with_message("Unterminated Raw Code")
					.with_label(
						Label::new((token.source().clone(), token.range.clone()))
						.with_message(format!("Missing terminating `{}` after first `{}`",
								"?}".fg(parser.colors().info),
								"{?".fg(parser.colors().info)))
						.with_color(parser.colors().error))
					.finish());
					return reports;
			}
			Some(content) => {
				let processed = util::process_escaped('\\', "?}",
					content.as_str().trim_start().trim_end());

				if processed.is_empty()
				{
					reports.push(
						Report::build(ReportKind::Warning, token.source(), content.start())
						.with_message("Empty Raw Code")
						.with_label(
							Label::new((token.source().clone(), content.range()))
							.with_message("Raw code is empty")
							.with_color(parser.colors().warning))
						.finish());
				}
				processed
			}
		};

		let properties = match matches.get(1)
		{
			None => match self.properties.default() {
				Ok(properties) => properties,
				Err(e) => {
					reports.push(
						Report::build(ReportKind::Error, token.source(), token.start())
						.with_message("Invalid Raw Code")
						.with_label(
							Label::new((token.source().clone(), token.range.clone()))
							.with_message(format!("Raw code is missing properties: {e}"))
							.with_color(parser.colors().error))
						.finish());
						return reports;
				},
			}		
			Some(props) => {
				let processed = util::process_escaped('\\', "]",
					props.as_str().trim_start().trim_end());
				match self.properties.parse(processed.as_str())
				{
					Err(e) => {
						reports.push(
							Report::build(ReportKind::Error, token.source(), props.start())
							.with_message("Invalid Raw Code Properties")
							.with_label(
								Label::new((token.source().clone(), props.range()))
								.with_message(e)
								.with_color(parser.colors().error))
							.finish());
						return reports;
					}
					Ok(properties) => properties
				}
			}
		};

		let raw_kind : ElemKind = match properties.get("kind",
			|prop, value| ElemKind::from_str(value.as_str()).map_err(|e| (prop, e)))
		{
			Ok((_prop, kind)) => kind,
			Err((prop, e)) => {
				reports.push(
					Report::build(ReportKind::Error, token.source(), token.start())
					.with_message("Invalid Raw Code Property")
					.with_label(
						Label::new((token.source().clone(), token.range.clone()))
						.with_message(format!("Property `kind: {}` cannot be converted: {}",
								prop.fg(parser.colors().info),
								e.fg(parser.colors().error)))
						.with_color(parser.colors().warning))
					.finish());
					return reports;
			}
		};

		parser.push(document, Box::new(Raw::new(
			token.clone(),
			raw_kind,
			raw_content
		)));

        reports
    }

	// TODO
	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Vec<(String, Function<'lua>)> { vec![] }
}
