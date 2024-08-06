use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::document::DocumentAccessors;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::parser::ParserState;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::state::RuleState;
use crate::parser::state::Scope;
use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
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

impl RuleState for StyleState {
	fn scope(&self) -> Scope { Scope::PARAGRAPH }

	fn on_remove<'a>(
		&self,
		state: &ParserState,
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

				reports.push(
					Report::build(ReportKind::Error, token.source(), token.start())
						.with_message("Unterminated Style")
						.with_label(
							Label::new((token.source(), token.range.clone()))
								.with_order(1)
								.with_message(format!(
									"Style {} starts here",
									name.fg(state.parser.colors().info)
								))
								.with_color(state.parser.colors().error),
						)
						.with_label(
							Label::new(paragraph_end)
								.with_order(1)
								.with_message(format!("Paragraph ends here"))
								.with_color(state.parser.colors().error),
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

static STATE_NAME: &'static str = "elements.style";

impl RegexRule for StyleRule {
	fn name(&self) -> &'static str { "Style" }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn on_regex_match(
		&self,
		index: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		_matches: Captures,
	) -> Vec<Report<(Rc<dyn Source>, Range<usize>)>> {
		let query = state.shared.rule_state.borrow().get(STATE_NAME);
		let style_state = match query {
			Some(state) => state,
			None => {
				// Insert as a new state
				match state
					.shared
					.rule_state
					.borrow_mut()
					.insert(STATE_NAME.into(), Rc::new(RefCell::new(StyleState::new())))
				{
					Err(_) => panic!("Unknown error"),
					Ok(state) => state,
				}
			}
		};

		if let Some(style_state) = style_state.borrow_mut().downcast_mut::<StyleState>() {
			style_state.toggled[index] = style_state.toggled[index]
				.clone()
				.map_or(Some(token.clone()), |_| None);
			state.push(
				document,
				Box::new(Style::new(
					token.clone(),
					index,
					!style_state.toggled[index].is_some(),
				)),
			);
		} else {
			panic!("Invalid state at `{STATE_NAME}`");
		}

		return vec![];
	}
}

#[cfg(test)]
mod tests {
	use crate::elements::text::Text;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;

	use super::*;

	#[test]
	fn parser() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
Some *style
terminated here*

**BOLD + *italic***
__`UNDERLINE+EM`__
"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let doc = parser.parse(ParserState::new(&parser, None), source, None);

		validate_document!(doc.content().borrow(), 0,
			Paragraph {
				Text;
				Style { kind == 1, close == false };
				Text;
				Style { kind == 1, close == true };
			};
			Paragraph {
				Style { kind == 0, close == false }; // **
				Text;
				Style { kind == 1, close == false }; // *
				Text;
				Style { kind == 0, close == true }; // **
				Style { kind == 1, close == true }; // *

				Style { kind == 2, close == false }; // __
				Style { kind == 3, close == false }; // `
				Text;
				Style { kind == 3, close == true }; // `
				Style { kind == 2, close == true }; // __
			};
		);
	}
}
