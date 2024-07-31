use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::document::DocumentAccessors;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::parser::Parser;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::state::Scope;
use crate::parser::state::State;
use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use lazy_static::lazy_static;
use mlua::Function;
use mlua::Lua;
use regex::Captures;
use regex::Regex;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use super::paragraph::Paragraph;

#[derive(Debug)]
pub struct Style {
	location: Token,
	kind: usize,
	close: bool,
}

impl Style {
	pub fn new(location: Token, kind: usize, close: bool) -> Self {
		Self {
			location,
			kind,
			close,
		}
	}
}

impl Element for Style {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Inline }
	fn element_name(&self) -> &'static str { "Section" }
	fn to_string(&self) -> String { format!("{self:#?}") }
	fn compile(&self, compiler: &Compiler, _document: &dyn Document) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				Ok([
					// Bold
					"<b>", "</b>", // Italic
					"<i>", "</i>", // Underline
					"<u>", "</u>", // Code
					"<em>", "</em>",
				][self.kind * 2 + self.close as usize]
					.to_string())
			}
			Target::LATEX => Err("Unimplemented compiler".to_string()),
		}
	}
}

struct StyleState {
	toggled: [Option<Token>; 4],
}

impl StyleState {
	const NAMES: [&'static str; 4] = ["Bold", "Italic", "Underline", "Code"];

	fn new() -> Self {
		Self {
			toggled: [None, None, None, None],
		}
	}
}

impl State for StyleState {
	fn scope(&self) -> Scope { Scope::PARAGRAPH }

	fn on_remove<'a>(
		&self,
		parser: &dyn Parser,
		document: &dyn Document,
	) -> Vec<Report<'a, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		self.toggled
			.iter()
			.zip(StyleState::NAMES)
			.for_each(|(token, name)| {
				if token.is_none() {
					return;
				} // Style not enabled
				let token = token.as_ref().unwrap();

				//let range = range.as_ref().unwrap();

				//let active_range = range.start .. paragraph.location().end()-1;

				let paragraph = document.last_element::<Paragraph>().unwrap();
				let paragraph_end = paragraph
					.content
					.last()
					.and_then(|last| {
						Some((
							last.location().source(),
							last.location().end() - 1..last.location().end(),
						))
					})
					.unwrap();

				// TODO: Allow style to span multiple documents if they don't break paragraph.
				reports.push(
					Report::build(ReportKind::Error, token.source(), token.start())
						.with_message("Unterminated style")
						//.with_label(
						//	Label::new((document.source(), active_range.clone()))
						//	.with_order(0)
						//	.with_message(format!("Style {} is not terminated before the end of paragraph",
						//	name.fg(parser.colors().info)))
						//	.with_color(parser.colors().error))
						.with_label(
							Label::new((token.source(), token.range.clone()))
								.with_order(1)
								.with_message(format!(
									"Style {} starts here",
									name.fg(parser.colors().info)
								))
								.with_color(parser.colors().info),
						)
						.with_label(
							Label::new(paragraph_end)
								.with_order(1)
								.with_message(format!("Paragraph ends here"))
								.with_color(parser.colors().info),
						)
						.with_note("Styles cannot span multiple documents (i.e @import)")
						.finish(),
				);
			});

		return reports;
	}
}

pub struct StyleRule {
	re: [Regex; 4],
}

impl StyleRule {
	pub fn new() -> Self {
		Self {
			re: [
				// Bold
				Regex::new(r"\*\*").unwrap(),
				// Italic
				Regex::new(r"\*").unwrap(),
				// Underline
				Regex::new(r"__").unwrap(),
				// Code
				Regex::new(r"`").unwrap(),
			],
		}
	}
}

lazy_static! {
	static ref STATE_NAME: String = "elements.style".to_string();
}

impl RegexRule for StyleRule {
	fn name(&self) -> &'static str { "Style" }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn on_regex_match(
		&self,
		index: usize,
		parser: &dyn Parser,
		document: &dyn Document,
		token: Token,
		_matches: Captures,
	) -> Vec<Report<(Rc<dyn Source>, Range<usize>)>> {
		let query = parser.state().query(&STATE_NAME);
		let state = match query {
			Some(state) => state,
			None => {
				// Insert as a new state
				match parser
					.state_mut()
					.insert(STATE_NAME.clone(), Rc::new(RefCell::new(StyleState::new())))
				{
					Err(_) => panic!("Unknown error"),
					Ok(state) => state,
				}
			}
		};

		if let Some(style_state) = state.borrow_mut().as_any_mut().downcast_mut::<StyleState>() {
			style_state.toggled[index] = style_state.toggled[index]
				.clone()
				.map_or(Some(token.clone()), |_| None);
			parser.push(
				document,
				Box::new(Style::new(
					token.clone(),
					index,
					!style_state.toggled[index].is_some(),
				)),
			);
		} else {
			panic!("Invalid state at `{}`", STATE_NAME.as_str());
		}

		return vec![];
	}

	// TODO
	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Option<Vec<(String, Function<'lua>)>> { None }
}
