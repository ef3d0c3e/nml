use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use mlua::Function;
use mlua::Lua;
use regex::Captures;
use regex::Match;
use regex::Regex;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::document::references::validate_refname;
use crate::parser::parser::Parser;
use crate::parser::parser::ReportColors;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::util;
use crate::parser::util::Property;
use crate::parser::util::PropertyMap;
use crate::parser::util::PropertyParser;

#[derive(Debug)]
pub struct Reference {
	pub(self) location: Token,
	pub(self) refname: String,
	pub(self) caption: Option<String>,
}

impl Reference {
	pub fn caption(&self) -> Option<&String> { self.caption.as_ref() }
}

impl Element for Reference {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Inline }

	fn element_name(&self) -> &'static str { "Reference" }

	fn to_string(&self) -> String { format!("{self:#?}") }

	fn compile(&self, compiler: &Compiler, document: &dyn Document) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				let elemref = document.get_reference(self.refname.as_str()).unwrap();
				let elem = document.get_from_reference(&elemref).unwrap();

				elem.compile_reference(
					compiler,
					document,
					self,
					compiler.reference_id(document, elemref),
				)
			}
			_ => todo!(""),
		}
	}
}

pub struct ReferenceRule {
	re: [Regex; 1],
	properties: PropertyParser,
}

impl ReferenceRule {
	pub fn new() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"caption".to_string(),
			Property::new(
				false,
				"Override the display of the reference".to_string(),
				None,
			),
		);
		Self {
			re: [Regex::new(r"§\{(.*)\}(\[((?:\\.|[^\\\\])*?)\])?").unwrap()],
			properties: PropertyParser{ properties: props },
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
						.with_message("Invalid Media Properties")
						.with_label(
							Label::new((token.source().clone(), token.range.clone()))
								.with_message(format!("Media is missing required property: {e}"))
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
							.with_message("Invalid Media Properties")
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

impl RegexRule for ReferenceRule {
	fn name(&self) -> &'static str { "Reference" }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn on_regex_match<'a>(
		&self,
		_: usize,
		parser: &dyn Parser,
		document: &'a (dyn Document<'a> + 'a),
		token: Token,
		matches: Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let refname = match (
			matches.get(1).unwrap(),
			validate_refname(document, matches.get(1).unwrap().as_str(), false),
		) {
			(m, Ok(refname)) => {
				if document.get_reference(refname).is_none() {
					reports.push(
						Report::build(ReportKind::Error, token.source(), m.start())
							.with_message("Uknown Reference Refname")
							.with_label(
								Label::new((token.source().clone(), m.range())).with_message(
									format!(
										"Could not find element with reference: `{}`",
										refname.fg(parser.colors().info)
									),
								),
							)
							.finish(),
					);
					return reports;
				}
				refname.to_string()
			}
			(m, Err(err)) => {
				reports.push(
					Report::build(ReportKind::Error, token.source(), m.start())
						.with_message("Invalid Reference Refname")
						.with_label(
							Label::new((token.source().clone(), m.range())).with_message(err),
						)
						.finish(),
				);
				return reports;
			}
		};
		// Properties
		let properties = match self.parse_properties(parser.colors(), &token, &matches.get(3)) {
			Ok(pm) => pm,
			Err(report) => {
				reports.push(report);
				return reports;
			}
		};

		let caption = properties
			.get("caption", |_, value| -> Result<String, ()> {
				Ok(value.clone())
			})
			.ok()
			.and_then(|(_, s)| Some(s));

		parser.push(
			document,
			Box::new(Reference {
				location: token,
				refname,
				caption,
			}),
		);

		reports
	}

	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Option<Vec<(String, Function<'lua>)>> { None }
}
