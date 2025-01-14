use std::any::Any;
use std::cell::Ref;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use ariadne::Fmt;
use lsp::conceal::ConcealTarget;
use lsp::conceal::Conceals;
use lsp::hints::Hints;
use lsp::semantic::Semantics;
use parser::rule::Rule;
use parser::source::Token;
use parser::source::VirtualSource;
use parser::util::parse_paragraph;
use regex::Regex;
use serde_json::json;

use crate::document::document::Document;
use crate::document::document::DocumentAccessors;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::reports::Report;
use crate::parser::source::Cursor;
use crate::parser::util::escape_source;

use super::elem::CheckboxState;
use super::elem::CustomListData;
use super::elem::ListEntry;
use super::elem::ListMarker;
use super::elem::MarkerKind;

fn push_markers(
	token: &Token,
	state: &ParserState,
	document: &dyn Document,
	current: &Vec<(bool, usize)>,
	target: &Vec<(bool, usize)>,
) {
	let mut start_pos = 0;
	for i in 0..std::cmp::min(target.len(), current.len()) {
		if current[i].0 != target[i].0 {
			break;
		}

		start_pos += 1;
	}

	// Close
	for i in start_pos..current.len() {
		state.push(
			document,
			Box::new(ListMarker {
				location: token.clone(),
				kind: MarkerKind::Close,
				numbered: current[current.len() - 1 - (i - start_pos)].0,
			}),
		);
	}

	// Open
	for i in start_pos..target.len() {
		state.push(
			document,
			Box::new(ListMarker {
				location: token.clone(),
				kind: MarkerKind::Open,
				numbered: target[i].0,
			}),
		);
	}
}

fn parse_depth(depth: &str, document: &dyn Document, offset: usize) -> Vec<(bool, usize)> {
	let mut parsed = vec![];
	let prev_entry = document
		.last_element::<ListEntry>()
		.and_then(|entry| Ref::filter_map(entry, |e| Some(&e.numbering)).ok());

	let mut continue_match = true;
	depth.chars().enumerate().for_each(|(idx, c)| {
		let number = if offset == usize::MAX || idx + 1 != depth.len() {
			prev_entry
				.as_ref()
				.and_then(|v| {
					if !continue_match {
						return None;
					}
					let numbered = c == '-';

					match v.get(idx) {
						None => None,
						Some((prev_numbered, prev_idx)) => {
							if *prev_numbered != numbered {
								continue_match = false;
								None
							}
							// New depth
							else if idx + 1 == v.len() {
								Some(*prev_idx + 1)
							}
							// Increase from previous
							else {
								Some(*prev_idx)
							} // Do nothing
						}
					}
				})
				.unwrap_or(1)
		} else {
			offset
		};

		match c {
			'*' => parsed.push((false, number)),
			'-' => parsed.push((true, number)),
			_ => panic!("Unimplemented"),
		}
	});

	parsed
}

#[auto_registry::auto_registry(registry = "rules")]
pub struct ListRule {
	start_re: Regex,
	continue_re: Regex,
	properties: PropertyParser,
}

impl Default for ListRule {
	fn default() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"offset".to_string(),
			Property::new("Entry numbering offset".to_string(), None),
		);
		props.insert(
			"bullet".to_string(),
			Property::new("Entry bullet".to_string(), None),
		);

		Self {
			start_re: Regex::new(r"(?:^|\n)(?:[^\S\r\n]+)([*-]+)(?:\[((?:\\.|[^\\\\])*?)\])?(?:[^\S\r\n]{0,1}\[((?:\\.|[^\\\\])*?)\])?(?:[^\S\r\n]+)(.*)")
				.unwrap(),
			continue_re: Regex::new(r"(?:^|\n)([^\S\r\n].*)").unwrap(),
			properties: PropertyParser { properties: props },
		}
	}
}

impl Rule for ListRule {
	fn name(&self) -> &'static str { "List" }

	fn previous(&self) -> Option<&'static str> { Some("Raw") }

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
		document: &'a dyn Document<'a>,
		cursor: Cursor,
		_match_data: Box<dyn Any>,
	) -> (Cursor, Vec<Report>) {
		let mut reports = vec![];

		let content = cursor.source.content();
		let mut end_cursor = cursor.clone();
		loop {
			if let Some(captures) = self.start_re.captures_at(content, end_cursor.pos) {
				if captures.get(0).unwrap().start() != end_cursor.pos {
					break;
				}
				// Advance cursor
				end_cursor = end_cursor.at(captures.get(0).unwrap().end());

				// Properties
				let prop_source = escape_source(
					end_cursor.source.clone(),
					captures.get(2).map_or(0..0, |m| m.range()),
					"List Properties".into(),
					'\\',
					"]",
				);
				let properties = match self.properties.parse(
					"List",
					&mut reports,
					state,
					Token::new(0..prop_source.content().len(), prop_source),
				) {
					Some(props) => props,
					None => return (end_cursor, reports),
				};

				let (offset, bullet) = match (
					properties.get_opt(&mut reports, "offset", |_, value| {
						value.value.parse::<usize>()
					}),
					properties.get_opt(&mut reports, "bullet", |_, value| {
						Result::<_, String>::Ok(value.value.clone())
					}),
				) {
					(Some(offset), Some(bullet)) => {
						// Get bullet from previous entry if it exists
						if bullet.is_none() {
							(
								offset,
								document
									.last_element::<ListEntry>()
									.and_then(|prev| prev.bullet.clone()),
							)
						} else {
							(offset, bullet)
						}
					}
					_ => return (end_cursor, reports),
				};

				// Depth
				let depth = parse_depth(
					captures.get(1).unwrap().as_str(),
					document,
					offset.unwrap_or(usize::MAX),
				);

				// Custom list data
				let custom_data = if let Some((custom_data, content)) =
					captures.get(3).map(|m| (m.range(), m.as_str()))
				{
					let data = match content {
						"" | " " => CustomListData::Checkbox(CheckboxState::Unchecked),
						"-" => CustomListData::Checkbox(CheckboxState::Partial),
						"x" | "X" => CustomListData::Checkbox(CheckboxState::Checked),
						_ => {
							report_err!(
								&mut reports,
								end_cursor.source.clone(),
								"Unknown custom list data".into(),
								span(
									custom_data.clone(),
									format!(
										"Cannot understand custom list data: `{}`",
										content.fg(state.parser.colors().highlight),
									)
								)
							);
							return (end_cursor, reports);
						}
					};

					// Add conceal
					if let Some(conceals) =
						Conceals::from_source(cursor.source.clone(), &state.shared.lsp)
					{
						match data {
							CustomListData::Checkbox(checkbox_state) => conceals.add(
								custom_data.start - 1..custom_data.end + 1,
								ConcealTarget::Token {
									token: "checkbox".into(),
									params: json!({
										"state": checkbox_state,
									}),
								},
							),
						}
					}
					Some(data)
				} else {
					None
				};

				// Semantic
				if let Some((sems, tokens)) =
					Semantics::from_source(cursor.source.clone(), &state.shared.lsp)
				{
					sems.add(captures.get(1).unwrap().range(), tokens.list_bullet);
					if let Some(props) = captures.get(2).map(|m| m.range()) {
						sems.add(props.start - 1..props.start, tokens.list_props_sep);
						sems.add(props.end..props.end + 1, tokens.list_props_sep);
					}
					if let Some(props) = captures.get(3).map(|m| m.range()) {
						sems.add(props, tokens.list_entry_type);
					}
				}

				if let Some(conceals) =
					Conceals::from_source(cursor.source.clone(), &state.shared.lsp)
				{
					let mut i = captures.get(1).unwrap().start();
					for (depth, (numbered, _)) in depth.iter().enumerate() {
						conceals.add(
							i..i + 1,
							lsp::conceal::ConcealTarget::Token {
								token: "bullet".into(),
								params: json!({
									"depth": depth,
									"numbered": *numbered,
								}),
							},
						);
						i += 1;
					}
				}

				// Hints
				if let Some(hints) = Hints::from_source(cursor.source.clone(), &state.shared.lsp) {
					let mut label = String::new();
					for (_, id) in &depth {
						if !label.is_empty() {
							label.push('.');
						}
						label.push_str(id.to_string().as_str());
					}
					hints.add(captures.get(1).unwrap().end(), label);
				}

				// Content
				let entry_start = captures.get(4).unwrap().start();
				let mut entry_content = captures.get(4).unwrap().as_str().to_string();
				while let Some(captures) = self.continue_re.captures_at(content, end_cursor.pos) {
					// Break if next element is another entry
					if captures.get(0).unwrap().start() != end_cursor.pos
						|| captures
							.get(1)
							.unwrap()
							.as_str()
							.find(['*', '-'])
							.map(|delim| {
								captures.get(1).unwrap().as_str()[0..delim]
									.chars()
									.all(|c| c.is_whitespace())
							}) == Some(true)
					{
						break;
					}
					// Advance cursor
					end_cursor = end_cursor.at(captures.get(0).unwrap().end());

					entry_content += "\n";
					entry_content += captures.get(1).unwrap().as_str();
				}

				// Parse entry content
				let token = Token::new(entry_start..end_cursor.pos, end_cursor.source.clone());
				let entry_src = Arc::new(VirtualSource::new(
					token.clone(),
					"List Entry".to_string(),
					entry_content,
				));
				let parsed_content = match parse_paragraph(state, entry_src, document) {
					Err(err) => {
						report_warn!(
							&mut reports,
							token.source(),
							"Unable to parse List Entry".into(),
							span(token.range.clone(), err.into())
						);
						// Return an empty paragraph
						vec![]
					}
					Ok(mut paragraph) => std::mem::take(&mut paragraph.content),
				};

				if let Some(previous_depth) = document
					.last_element::<ListEntry>()
					.map(|ent| ent.numbering.clone())
				{
					push_markers(&token, state, document, &previous_depth, &depth);
				} else {
					push_markers(&token, state, document, &vec![], &depth);
				}

				state.push(
					document,
					Box::new(ListEntry {
						location: Token::new(
							entry_start..end_cursor.pos,
							end_cursor.source.clone(),
						),
						numbering: depth,
						content: parsed_content,
						bullet,
						custom: custom_data,
					}),
				);
			} else {
				break;
			}
		}

		// Close all lists
		let current = document
			.last_element::<ListEntry>()
			.map(|ent| ent.numbering.clone())
			.unwrap();
		let token = Token::new(end_cursor.pos..end_cursor.pos, end_cursor.source.clone());
		push_markers(&token, state, document, &current, &Vec::new());

		(end_cursor, reports)
	}
}
