use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use std::any::Any;
use std::rc::Rc;

use ariadne::Fmt;
use document::element::ElemKind;
use document::element::Element;
use elements::list::ListEntry;
use elements::list::ListMarker;
use elements::paragraph::Paragraph;
use elements::text::Text;
use lsp::conceal::Conceals;
use lsp::semantic::Semantics;
use parser::parser::SharedState;
use parser::source::Token;
use parser::source::VirtualSource;
use parser::util::escape_source;
use regex::Regex;
use serde_json::json;

use crate::document::document::Document;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::reports::Report;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;

use super::elem::Block;
use super::style::QuoteStyle;

#[auto_registry::auto_registry(registry = "rules")]
pub struct BlockRule {
	start_re: Regex,
	continue_re: Regex,
}

impl BlockRule {
	pub fn new() -> Self {
		Self {
			start_re: Regex::new(
				r"(?:^|\n)>[^\S\r\n]*(?:\[!((?:\\.|[^\\\\])*?)\])(?:\[((?:\\.|[^\\\\])*?)\])?[^\S\r\n]*",
			)
			.unwrap(),
			continue_re: Regex::new(r"(?:^|\n)>(.*)").unwrap(),
		}
	}
}

impl Rule for BlockRule {
	fn name(&self) -> &'static str { "Block" }

	fn previous(&self) -> Option<&'static str> { Some("List") }

	fn next_match(
		&self,
		mode: &ParseMode,
		_state: &ParserState,
		cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any>)> {
		if mode.paragraph_only {
			return None;
		}
		self.start_re
			.find_at(cursor.source.content(), cursor.pos)
			.map(|m| (m.start(), Box::new([false; 0]) as Box<dyn Any>))
	}

	fn on_match<'a>(
		&self,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
		cursor: Cursor,
		_match_data: Box<dyn Any>,
	) -> (Cursor, Vec<Report>) {
		let mut reports = vec![];

		let content = cursor.source.content();
		let mut end_cursor = cursor.clone();

		let captures = if let Some(result) = self.start_re.captures_at(content, end_cursor.pos) {
			if result.get(0).unwrap().start() != end_cursor.pos {
				return (end_cursor, reports);
			}
			result
		} else {
			return (end_cursor, reports);
		};

		// Advance cursor
		end_cursor = end_cursor.at(captures.get(0).unwrap().end());

		// Get Block type
		let block_name = captures.get(1).unwrap();
		let trimmed_block_name = block_name.as_str().trim_start().trim_end();
		let block_type = match state.shared.blocks.borrow().get(trimmed_block_name) {
			None => {
				report_err!(
					&mut reports,
					cursor.source.clone(),
					"Unknown Block".into(),
					span(
						block_name.range(),
						format!(
							"Cannot find block type `{}`",
							trimmed_block_name.fg(state.parser.colors().highlight)
						)
					)
				);
				return (end_cursor, reports);
			}
			Some(block_type) => block_type,
		};

		// Properties
		let prop_source = escape_source(
			cursor.source.clone(),
			captures.get(2).map_or(0..0, |m| m.range()),
			"Block Properties".into(),
			'\\',
			"]",
		);
		let properties = if let Some(props) = block_type.parse_properties(
			&mut reports,
			state,
			Token::new(0..prop_source.content().len(), prop_source.clone()),
		) {
			props
		} else {
			return (end_cursor, reports);
		};

		// Semantics
		if let Some((sems, tokens)) =
			Semantics::from_source(cursor.source.clone(), &state.shared.lsp)
		{
			let range = captures.get(0).unwrap().range();
			let start = if content.as_bytes()[range.start] == b'\n' {
				range.start + 1
			} else {
				range.start
			};
			sems.add(start..start + 1, tokens.block_marker);
			let name_range = captures.get(1).unwrap().range();
			sems.add(name_range.start - 2..name_range.end + 1, tokens.block_name);
			if let Some(props) = captures.get(2).map(|m| m.range()) {
				sems.add(props.start - 1..props.start, tokens.block_props_sep);
				sems.add(props.end..props.end + 1, tokens.block_props_sep);
			}
		}

		// Conceals
		if let Some(conceals) = Conceals::from_source(cursor.source.clone(), &state.shared.lsp) {
			let range = captures.get(0).unwrap().range();
			let start = if content.as_bytes()[range.start] == b'\n' {
				range.start + 1
			} else {
				range.start
			};
			conceals.add(
				start..start + 1,
				lsp::conceal::ConcealTarget::Token {
					token: "block".into(),
					params: json!({
						"name": block_type.name().to_string(),
					}),
				},
			);
			let name_range = captures.get(1).unwrap().range();
			conceals.add(
				name_range.start - 2..name_range.end + 1,
				lsp::conceal::ConcealTarget::Token {
					token: "block_name".into(),
					params: json!({
						"name": block_type.name().to_string(),
					}),
				},
			)
		}

		// Content
		let entry_start = captures.get(0).unwrap().end();
		let mut entry_content = String::new();
		let mut offsets = vec![];
		while let Some(captures) = self.continue_re.captures_at(content, end_cursor.pos) {
			if captures.get(0).unwrap().start() != end_cursor.pos {
				break;
			}
			// Advance cursor
			end_cursor = end_cursor.at(captures.get(0).unwrap().end());
			// Offset
			let last = offsets.last().map_or(0, |(_, last)| *last);
			offsets.push((
				entry_content.len() + 1,
				last + (captures.get(1).unwrap().start() - captures.get(0).unwrap().start() - 1)
					as isize,
			));

			entry_content += "\n";
			entry_content += captures.get(1).unwrap().as_str();

			// Semantics
			if let Some((sems, tokens)) =
				Semantics::from_source(cursor.source.clone(), &state.shared.lsp)
			{
				let range = captures.get(0).unwrap().range();
				let start = if content.as_bytes()[range.start] == b'\n' {
					range.start + 1
				} else {
					range.start
				};
				sems.add_to_queue(start..start + 1, tokens.block_marker);
			}

			// Conceals
			if let Some(conceals) = Conceals::from_source(cursor.source.clone(), &state.shared.lsp)
			{
				let range = captures.get(0).unwrap().range();
				let start = if content.as_bytes()[range.start] == b'\n' {
					range.start + 1
				} else {
					range.start
				};
				conceals.add(
					start..start + 1,
					lsp::conceal::ConcealTarget::Token {
						token: "block".into(),
						params: json!({
							"name": block_type.name().to_string(),
						}),
					},
				);
			}
		}

		// Parse entry content
		let token = Token::new(entry_start..end_cursor.pos, end_cursor.source.clone());
		let entry_src = Rc::new(VirtualSource::new_offsets(
			token.clone(),
			"Block Entry".to_string(),
			entry_content,
			offsets,
		));
		// Parse content
		let parsed_doc = state.with_state(|new_state| {
			new_state
				.parser
				.parse(new_state, entry_src, Some(document), ParseMode::default())
				.0
		});

		// Extract paragraph and nested blockquotes
		let mut parsed_content: Vec<Box<dyn Element>> = vec![];
		for mut elem in parsed_doc.content().borrow_mut().drain(..) {
			if let Some(paragraph) = elem.downcast_mut::<Paragraph>() {
				if let Some(last) = parsed_content.last() {
					if last.kind() == ElemKind::Inline {
						parsed_content.push(Box::new(Text {
							location: Token::new(
								last.location().end()..last.location().end(),
								last.location().source(),
							),
							content: " ".to_string(),
						}) as Box<dyn Element>);
					}
				}
				parsed_content.extend(std::mem::take(&mut paragraph.content));
			} else if elem.downcast_ref::<Block>().is_some()
				|| elem.downcast_ref::<ListEntry>().is_some()
				|| elem.downcast_ref::<ListMarker>().is_some()
			{
				parsed_content.push(elem);
			} else {
				report_err!(
					&mut reports,
					token.source(),
					"Unable to Parse Block Entry".into(),
					span(
						token.range.clone(),
						"Block may only contain paragraphs and other blocks".into()
					)
				);
				return (end_cursor, reports);
			}
		}

		state.push(
			document,
			Box::new(Block {
				location: Token::new(entry_start..end_cursor.pos, end_cursor.source.clone()),
				content: parsed_content,
				block_type,
				block_properties: properties,
			}),
		);

		(end_cursor, reports)
	}

	fn register_shared_state(&self, state: &SharedState) {
		let mut holder = state.blocks.borrow_mut();
		holder.insert(Rc::new(super::custom::Quote::default()));
		holder.insert(Rc::new(super::custom::Warning::default()));
		holder.insert(Rc::new(super::custom::Note::default()));
		holder.insert(Rc::new(super::custom::Todo::default()));
		holder.insert(Rc::new(super::custom::Tip::default()));
		holder.insert(Rc::new(super::custom::Caution::default()));

		let mut holder = state.styles.borrow_mut();
		holder.set_current(Rc::new(QuoteStyle::default()));
	}
}
