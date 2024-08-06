use std::cell::RefCell;
use std::rc::Rc;

use crate::document::document::Document;
use crate::document::element::DocumentEnd;
use crate::document::langdocument::LangDocument;
use crate::elements::registrar::register;
use crate::elements::text::Text;

use super::parser::Parser;
use super::parser::ParserState;
use super::parser::ReportColors;
use super::rule::Rule;
use super::source::Cursor;
use super::source::Source;
use super::source::Token;
use super::util;

/// Parser for the language
#[derive(Debug)]
pub struct LangParser {
	rules: Vec<Box<dyn Rule>>,
	colors: ReportColors,

	// Parser state
	pub err_flag: RefCell<bool>,
}

impl LangParser {
	pub fn default() -> Self {
		let mut s = Self {
			rules: vec![],
			colors: ReportColors::with_colors(),
			err_flag: RefCell::new(false),
		};

		// Register rules
		// TODO: use https://docs.rs/inventory/latest/inventory/
		register(&mut s);

		s
	}
}

impl Parser for LangParser {
	fn colors(&self) -> &ReportColors { &self.colors }

	fn rules(&self) -> &Vec<Box<dyn Rule>> { &self.rules }
	fn rules_mut(&mut self) -> &mut Vec<Box<dyn Rule>> { &mut self.rules }

	fn has_error(&self) -> bool { *self.err_flag.borrow() }

	fn parse<'a>(
		&self,
		state: ParserState,
		source: Rc<dyn Source>,
		parent: Option<&'a dyn Document<'a>>,
	) -> Box<dyn Document<'a> + 'a> {
		let doc = LangDocument::new(source.clone(), parent);

		let content = source.content();
		let mut cursor = Cursor::new(0usize, doc.source()); // Cursor in file

		if let Some(parent) = parent
		// Terminate parent's paragraph state
		{
			self.handle_reports(state.shared.rule_state.borrow_mut().on_scope_end(
				&state,
				parent,
				super::state::Scope::PARAGRAPH,
			));
		}

		loop {
			let (rule_pos, mut result) = state.update_matches(&cursor);

			// Unmatched content
			let text_content =
				util::process_text(&doc, &content.as_str()[cursor.pos..rule_pos.pos]);
			if !text_content.is_empty() {
				state.push(
					&doc,
					Box::new(Text::new(
						Token::new(cursor.pos..rule_pos.pos, source.clone()),
						text_content,
					)),
				);
			}

			if let Some((rule_index, match_data)) = result.take() {
				// Rule callback
				let dd: &'a dyn Document = unsafe { std::mem::transmute(&doc as &dyn Document) };
				let (new_cursor, reports) =
					self.rules[rule_index].on_match(&state, dd, rule_pos, match_data);

				self.handle_reports(reports);

				// Advance
				cursor = new_cursor;
			} else
			// No rules left
			{
				break;
			}
		}

		// Rule States

		self.handle_reports(state.shared.rule_state.borrow_mut().on_scope_end(
			&state,
			&doc,
			super::state::Scope::DOCUMENT,
		));

		state.push(
			&doc,
			Box::new(DocumentEnd(Token::new(
				doc.source().content().len()..doc.source().content().len(),
				doc.source(),
			))),
		);

		return Box::new(doc);
	}

	fn parse_into<'a>(
		&self,
		state: ParserState,
		source: Rc<dyn Source>,
		document: &'a dyn Document<'a>,
	) {
		let content = source.content();
		let mut cursor = Cursor::new(0usize, source.clone());

		loop {
			let (rule_pos, mut result) = state.update_matches(&cursor);

			// Unmatched content
			let text_content =
				util::process_text(document, &content.as_str()[cursor.pos..rule_pos.pos]);
			if !text_content.is_empty() {
				state.push(
					document,
					Box::new(Text::new(
						Token::new(cursor.pos..rule_pos.pos, source.clone()),
						text_content,
					)),
				);
			}

			if let Some((rule_index, match_data)) = result.take() {
				// Rule callback
				let (new_cursor, reports) =
					self.rules[rule_index].on_match(&state, document, rule_pos, match_data);

				self.handle_reports(reports);

				// Advance
				cursor = new_cursor;
			} else
			// No rules left
			{
				break;
			}
		}

		// State
		//self.handle_reports(source.clone(),
		//	self.state_mut().on_scope_end(&self, &document, super::state::Scope::DOCUMENT));

		//return doc;
	}
}
