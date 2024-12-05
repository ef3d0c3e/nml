use std::any::Any;
use std::cell::Ref;
use std::collections::HashMap;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::document::DocumentAccessors;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::lsp::semantic::Semantics;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::reports::macros::*;
use crate::parser::reports::Report;
use crate::parser::reports::*;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use crate::parser::util;
use lsp::conceal::Conceals;
use lsp::hints::Hints;
use parser::util::escape_source;
use regex::Regex;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MarkerKind {
	Open,
	Close,
}

#[derive(Debug)]
pub struct ListMarker {
	pub(self) location: Token,
	pub(self) numbered: bool,
	pub(self) kind: MarkerKind,
}

impl Element for ListMarker {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Block }

	fn element_name(&self) -> &'static str { "List Marker" }

	fn compile(
		&self,
		compiler: &Compiler,
		_document: &dyn Document,
		_cursor: usize,
	) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => match (self.kind, self.numbered) {
				(MarkerKind::Close, true) => Ok("</ol>".to_string()),
				(MarkerKind::Close, false) => Ok("</ul>".to_string()),
				(MarkerKind::Open, true) => Ok("<ol>".to_string()),
				(MarkerKind::Open, false) => Ok("<ul>".to_string()),
			},
			_ => todo!(),
		}
	}
}

#[derive(Debug)]
pub struct ListEntry {
	pub(self) location: Token,
	pub(self) numbering: Vec<(bool, usize)>,
	pub(self) content: Vec<Box<dyn Element>>,
	pub(self) bullet: Option<String>,
}

impl Element for ListEntry {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Block }

	fn element_name(&self) -> &'static str { "List Entry" }

	fn compile(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		cursor: usize,
	) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				let mut result = String::new();
				if let Some((numbered, number)) = self.numbering.last() {
					if *numbered {
						result += format!("<li value=\"{number}\">").as_str();
					} else {
						result += "<li>";
					}
				}
				for elem in &self.content {
					result += elem
						.compile(compiler, document, cursor + result.len())?
						.as_str();
				}
				result += "</li>";
				Ok(result)
			}
			_ => todo!(),
		}
	}

	fn as_container(&self) -> Option<&dyn ContainerElement> { Some(self) }
}

impl ContainerElement for ListEntry {
	fn contained(&self) -> &Vec<Box<dyn Element>> { &self.content }

	fn push(&mut self, elem: Box<dyn Element>) -> Result<(), String> {
		if elem.kind() == ElemKind::Block {
			return Err("Cannot add block element inside a list".to_string());
		}

		self.content.push(elem);
		Ok(())
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::list")]
pub struct ListRule {
	start_re: Regex,
	continue_re: Regex,
	properties: PropertyParser,
}

impl ListRule {
	pub fn new() -> Self {
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
			start_re: Regex::new(r"(?:^|\n)(?:[^\S\r\n]+)([*-]+)(?:\[((?:\\.|[^\\\\])*?)\])?(.*)")
				.unwrap(),
			continue_re: Regex::new(r"(?:^|\n)([^\S\r\n].*)").unwrap(),
			properties: PropertyParser { properties: props },
		}
	}

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
				let depth = ListRule::parse_depth(
					captures.get(1).unwrap().as_str(),
					document,
					offset.unwrap_or(usize::MAX),
				);

				// Semantic
				if let Some((sems, tokens)) =
					Semantics::from_source(cursor.source.clone(), &state.shared.lsp)
				{
					sems.add(captures.get(1).unwrap().range(), tokens.list_bullet);
					if let Some(props) = captures.get(2).map(|m| m.range()) {
						sems.add(props.start - 1..props.start, tokens.list_props_sep);
						sems.add(props.end..props.end + 1, tokens.list_props_sep);
					}
				}

				if let Some(conceals) =
					Conceals::from_source(cursor.source.clone(), &state.shared.lsp)
				{
					let mut i = captures.get(1).unwrap().start();
					for (numbered, _) in &depth {
						conceals.add(
							i..i + 1,
							lsp::conceal::ConcealTarget::Highlight {
								text: if *numbered {
									"⦾".into()
								} else {
									"⦿".into()
								},
								highlight_group: "Function".into(),
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
				let entry_start = captures.get(3).unwrap().start();
				let mut entry_content = captures.get(3).unwrap().as_str().to_string();
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
				let entry_src = Rc::new(VirtualSource::new(
					token.clone(),
					"List Entry".to_string(),
					entry_content,
				));
				let parsed_content = match util::parse_paragraph(state, entry_src, document) {
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
					ListRule::push_markers(&token, state, document, &previous_depth, &depth);
				} else {
					ListRule::push_markers(&token, state, document, &vec![], &depth);
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
		ListRule::push_markers(&token, state, document, &current, &Vec::new());

		(end_cursor, reports)
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
	use crate::validate_semantics;

	#[test]
	fn parser() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
 * 1
 *[offset=7] 2
	continued
 * 3

 * New list
 *-[bullet=(*)] A
 *- B
 * Back
 *-* More nested
"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let state = ParserState::new(&parser, None);
		let (doc, _) = parser.parse(state, source, None, ParseMode::default());

		validate_document!(doc.content().borrow(), 0,
			ListMarker { numbered == false, kind == MarkerKind::Open };
			ListEntry { numbering == vec![(false, 1)] } {
				Text { content == "1" };
			};
			ListEntry { numbering == vec![(false, 7)] } {
				Text { content == "2 continued" };
			};
			ListEntry { numbering == vec![(false, 8)] } {
				Text { content == "3" };
			};
			ListMarker { numbered == false, kind == MarkerKind::Close };

			Paragraph;

			ListMarker { numbered == false, kind == MarkerKind::Open };
			ListEntry { numbering == vec![(false, 1)] } {
				Text { content == "New list" };
			};
			ListMarker { numbered == true, kind == MarkerKind::Open };
				ListEntry { numbering == vec![(false, 2), (true, 1)], bullet == Some("(*)".to_string()) } {
					Text { content == "A" };
				};
				ListEntry { numbering == vec![(false, 2), (true, 2)], bullet == Some("(*)".to_string()) } {
					Text { content == "B" };
				};
			ListMarker { numbered == true, kind == MarkerKind::Close };
			ListEntry { numbering == vec![(false, 2)] } {
				Text { content == "Back" };
			};
			ListMarker { numbered == true, kind == MarkerKind::Open };
			ListMarker { numbered == false, kind == MarkerKind::Open };
			ListEntry { numbering == vec![(false, 3), (true, 1), (false, 1)] } {
				Text { content == "More nested" };
			};
			ListMarker { numbered == false, kind == MarkerKind::Close };
			ListMarker { numbered == true, kind == MarkerKind::Close };
			ListMarker { numbered == false, kind == MarkerKind::Close };
		);
	}

	#[test]
	fn semantic() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
 *[offset=5] First **bold**
	Second line
 *- Another
		"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (_, state) = parser.parse(
			ParserState::new_with_semantics(&parser, None),
			source.clone(),
			None,
			ParseMode::default(),
		);
		validate_semantics!(state, source.clone(), 0,
			list_bullet { delta_line == 1, delta_start == 1, length == 1 };
			list_props_sep { delta_line == 0, delta_start == 1, length == 1 };
			prop_name { delta_line == 0, delta_start == 1, length == 6 };
			prop_equal { delta_line == 0, delta_start == 6, length == 1 };
			prop_value { delta_line == 0, delta_start == 1, length == 1 };
			list_props_sep { delta_line == 0, delta_start == 1, length == 1 };
			style_marker { delta_line == 0, delta_start == 8, length == 2 };
			style_marker { delta_line == 0, delta_start == 6, length == 2 };
			list_bullet { delta_line == 2, delta_start == 1, length == 2 };
		);
	}
}
