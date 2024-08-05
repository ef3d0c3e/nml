use std::cell::RefCell;
use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

use ariadne::Label;
use ariadne::Report;

use crate::document::document::Document;
use crate::document::document::DocumentAccessors;
use crate::document::element::ContainerElement;
use crate::document::element::DocumentEnd;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::document::langdocument::LangDocument;
use crate::elements::paragraph::Paragraph;
use crate::elements::registrar::register;
use crate::elements::text::Text;
use crate::parser::source::SourceFile;
use crate::parser::source::VirtualSource;

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
			//matches: RefCell::new(Vec::new()),
			//state: RefCell::new(StateHolder::new()),
			//kernels: RefCell::new(HashMap::new()),
			//styles: RefCell::new(HashMap::new()),
			//layouts: RefCell::new(HashMap::new()),
			//custom_styles: RefCell::new(HashMap::new()),
		};
		// Register rules
		// TODO2: use https://docs.rs/inventory/latest/inventory/
		register(&mut s);


		s
	}
}

impl Parser for LangParser {
	fn colors(&self) -> &ReportColors { &self.colors }

	fn rules(&self) -> &Vec<Box<dyn Rule>> { &self.rules }

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
			Parser::handle_reports(&self,
				parent.source(),
				state.shared.rule_state
					.on_scope_end(self, parent, super::state::Scope::PARAGRAPH),
			);
		}

		loop {
			let (rule_pos, mut result) = state.update_matches(&cursor);

			// Unmatched content
			let text_content =
				util::process_text(&doc, &content.as_str()[cursor.pos..rule_pos.pos]);
			if !text_content.is_empty() {
				self.push(
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
				let (new_cursor, reports) = self.rules[rule_index].on_match(self, dd, rule_pos, match_data);

				self.handle_reports(doc.source(), reports);

				// Advance
				cursor = new_cursor;
			} else
			// No rules left
			{
				break;
			}
		}

		// State
		self.handle_reports(
			doc.source(),
			state.shared.rule_state
				.on_scope_end(&mut state, &doc, super::state::Scope::DOCUMENT),
		);

		self.push(
			&doc,
			Box::new(DocumentEnd(Token::new(
				doc.source().content().len()..doc.source().content().len(),
				doc.source(),
			))),
		);

		return Box::new(doc);
	}

	fn parse_into<'a>(&self, state: mut ParserState, source: Rc<dyn Source>, document: &'a dyn Document<'a>) {
		let content = source.content();
		let mut cursor = Cursor::new(0usize, source.clone());

		loop {
			let (rule_pos, mut result) = state.update_matches(&cursor);

			// Unmatched content
			let text_content =
				util::process_text(document, &content.as_str()[cursor.pos..rule_pos.pos]);
			if !text_content.is_empty() {
				self.push(
					document,
					Box::new(Text::new(
						Token::new(cursor.pos..rule_pos.pos, source.clone()),
						text_content,
					)),
				);
			}

			if let Some((rule_index, match_data)) = result.take() {
				// Rule callback
				let (new_cursor, reports) = self.rules[rule_index].on_match(&mut state, document, rule_pos, match_data);

				self.handle_reports(document.source(), reports);

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
