use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::document::document::Document;
use crate::document::element::DocumentEnd;
use crate::document::langdocument::LangDocument;
use crate::elements::text::Text;
use crate::lsp::definition::DefinitionData;
use crate::lsp::hints::HintsData;
use crate::lsp::semantic::Semantics;
use crate::lsp::semantic::SemanticsData;

use super::parser::ParseMode;
use super::parser::Parser;
use super::parser::ParserState;
use super::parser::ReportColors;
use super::reports::Report;
use super::rule::Rule;
use super::source::Cursor;
use super::source::Source;
use super::source::SourceFile;
use super::source::SourcePosition;
use super::source::Token;
use super::util;

/// Parser for the language
pub struct LangParser<'a> {
	rules: Vec<Box<dyn Rule>>,
	colors: ReportColors,
	report_handler: Box<dyn Fn(&ReportColors, Vec<Report>) + 'a>,

	// Parser state
	pub err_flag: RefCell<bool>,
}

impl<'a> LangParser<'a> {
	pub fn default() -> Self {
		let mut s = Self {
			rules: vec![],
			colors: ReportColors::with_colors(),
			err_flag: RefCell::new(false),
			report_handler: Box::new(Report::reports_to_stdout),
		};

		// Register rules
		for rule in super::rule::get_rule_registry() {
			s.add_rule(rule).unwrap();
		}

		s
	}

	pub fn new(
		with_colors: bool,
		report_handler: Box<dyn Fn(&ReportColors, Vec<Report>) + 'a>,
	) -> Self {
		let mut s = Self {
			rules: vec![],
			colors: if with_colors {
				ReportColors::with_colors()
			} else {
				ReportColors::without_colors()
			},
			err_flag: RefCell::new(false),
			report_handler,
		};

		// Register rules
		for rule in super::rule::get_rule_registry() {
			s.add_rule(rule).unwrap();
		}

		s
	}
}

impl<'b> Parser for LangParser<'b> {
	fn colors(&self) -> &ReportColors { &self.colors }

	fn rules(&self) -> &Vec<Box<dyn Rule>> { &self.rules }
	fn rules_mut(&mut self) -> &mut Vec<Box<dyn Rule>> { &mut self.rules }

	fn has_error(&self) -> bool { *self.err_flag.borrow() }

	fn parse<'p, 'a, 'doc>(
		&'p self,
		state: ParserState<'p, 'a>,
		source: Rc<dyn Source>,
		parent: Option<&'doc dyn Document<'doc>>,
		mode: ParseMode,
	) -> (Box<dyn Document<'doc> + 'doc>, ParserState<'p, 'a>) {
		let doc = LangDocument::new(source.clone(), parent);

		let current_dir = match std::env::current_dir() {
			Ok(dir) => dir,
			Err(err) => {
				eprintln!("Unable to get the current working directory: {err}");
				return (Box::new(doc), state);
			}
		};

		let path = source
			.original_position(0)
			.0
			.downcast_rc::<SourceFile>()
			.ok()
			.map(|source| {
				if source.path().is_empty()
				// Test mode
				{
					None
				} else {
					let start = if source.path().starts_with("file:///") {
						7
					} else {
						0
					};
					let mut path = PathBuf::from(&source.path()[start..]);
					match path.canonicalize() {
						Ok(cano) => path = cano,
						Err(err) => {
							eprintln!("Failed to canonicalize path `{}`: {err}", source.path())
						}
					}
					path.pop();
					Some(path)
				}
			})
			.unwrap();
		if let Some(path) = &path {
			if let Err(err) = std::env::set_current_dir(&path) {
				eprintln!(
					"Failed to set working directory to `{}`: {err}",
					path.to_str().unwrap_or("")
				);
			}
		}

		// Insert lsp data into state
		if let (Some(_), Some(lsp)) = (
			source.clone().downcast_rc::<SourceFile>().ok(),
			state.shared.lsp.as_ref(),
		) {
			lsp.borrow_mut().new_source(source.clone());
		}

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
			let (rule_pos, mut result) = state.update_matches(&mode, &cursor);

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

		// Process the end of the semantics queue
		Semantics::on_document_end(&state.shared.lsp, source.clone());

		if parent.is_none() {
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
		} else {
			self.handle_reports(state.shared.rule_state.borrow_mut().on_scope_end(
				&state,
				&doc,
				super::state::Scope::PARAGRAPH,
			));
		}

		if path.is_some() {
			if let Err(err) = std::env::set_current_dir(&current_dir) {
				println!(
					"Failed to set working directory to `{}`: {err} {source:#?}",
					current_dir.to_str().unwrap_or("")
				);
			}
		}

		(Box::new(doc), state)
	}

	fn parse_into<'p, 'a, 'doc>(
		&'p self,
		state: ParserState<'p, 'a>,
		source: Rc<dyn Source>,
		document: &'doc dyn Document<'doc>,
		mode: ParseMode,
	) -> ParserState<'p, 'a> {
		let content = source.content();
		let mut cursor = Cursor::new(0usize, source.clone());

		loop {
			let (rule_pos, mut result) = state.update_matches(&mode, &cursor);

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

		state
		// State
		//self.handle_reports(source.clone(),
		//	self.state_mut().on_scope_end(&self, &document, super::state::Scope::DOCUMENT));

		//return doc;
	}

	/// Handles the reports produced by parsing.
	fn handle_reports(&self, reports: Vec<Report>) {
		(self.report_handler)(self.colors(), reports);
	}
}
