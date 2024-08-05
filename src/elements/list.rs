use std::any::Any;
use std::cell::Ref;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::document::DocumentAccessors;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::parser::Parser;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use crate::parser::util;
use crate::parser::util::process_escaped;
use crate::parser::util::Property;
use crate::parser::util::PropertyMapError;
use crate::parser::util::PropertyParser;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use mlua::Function;
use mlua::Lua;
use regex::Match;
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

	fn compile(&self, compiler: &Compiler, _document: &dyn Document) -> Result<String, String> {
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

	fn compile(&self, compiler: &Compiler, document: &dyn Document) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				let mut result = "<li>".to_string();
				for elem in &self.content {
					result += elem.compile(compiler, document)?.as_str();
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
			Property::new(false, "Entry numbering offset".to_string(), None),
		);
		props.insert(
			"bullet".to_string(),
			Property::new(false, "Entry bullet".to_string(), None),
		);

		Self {
			start_re: Regex::new(r"(?:^|\n)(?:[^\S\r\n]+)([*-]+)(?:\[((?:\\.|[^\\\\])*?)\])?(.*)")
				.unwrap(),
			continue_re: Regex::new(r"(?:^|\n)([^\S\r\n]+)([^\s].*)").unwrap(),
			properties: PropertyParser { properties: props },
		}
	}

	fn push_markers(
		token: &Token,
		parser: &dyn Parser,
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
			parser.push(
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
			parser.push(
				document,
				Box::new(ListMarker {
					location: token.clone(),
					kind: MarkerKind::Open,
					numbered: target[i].0,
				}),
			);
		}
	}

	fn parse_properties(&self, m: Match) -> Result<(Option<usize>, Option<String>), String> {
		let processed = process_escaped('\\', "]", m.as_str());
		let pm = self.properties.parse(processed.as_str())?;

		let offset = match pm.get("offset", |_, s| s.parse::<usize>()) {
			Ok((_, val)) => Some(val),
			Err(err) => match err {
				PropertyMapError::ParseError(err) => {
					return Err(format!("Failed to parse `offset`: {err}"))
				}
				PropertyMapError::NotFoundError(_) => None,
			},
		};

		let bullet = pm
			.get("bullet", |_, s| -> Result<String, ()> { Ok(s.to_string()) })
			.map(|(_, s)| s)
			.ok();

		Ok((offset, bullet))
	}

	fn parse_depth(depth: &str, document: &dyn Document, offset: usize) -> Vec<(bool, usize)> {
		let mut parsed = vec![];
		// FIXME: Previous iteration used to recursively retrieve the list indent
		let prev_entry = document
			.last_element::<ListEntry>()
			.and_then(|entry| Ref::filter_map(entry, |e| Some(&e.numbering)).ok());

		let mut continue_match = true;
		depth.chars().enumerate().for_each(|(idx, c)| {
			let number = if offset == 0 {
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
									Some(prev_idx + 1)
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

		return parsed;
	}
}

impl Rule for ListRule {
	fn name(&self) -> &'static str { "List" }

	fn next_match(&self, _parser: &dyn Parser, cursor: &Cursor) -> Option<(usize, Box<dyn Any>)> {
		self.start_re
			.find_at(cursor.source.content(), cursor.pos)
			.map_or(None, |m| {
				Some((m.start(), Box::new([false; 0]) as Box<dyn Any>))
			})
	}

	fn on_match<'a>(
		&self,
		parser: &dyn Parser,
		document: &'a dyn Document<'a>,
		cursor: Cursor,
		_match_data: Option<Box<dyn Any>>,
	) -> (Cursor, Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>) {
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
				let mut offset = None;
				let mut bullet = None;
				if let Some(properties) = captures.get(2) {
					match self.parse_properties(properties) {
						Err(err) => {
							reports.push(
								Report::build(
									ReportKind::Warning,
									cursor.source.clone(),
									properties.start(),
								)
								.with_message("Invalid List Entry Properties")
								.with_label(
									Label::new((cursor.source.clone(), properties.range()))
										.with_message(err)
										.with_color(parser.colors().warning),
								)
								.finish(),
							);
							break;
						}
						Ok(props) => (offset, bullet) = props,
					}
				}
				// Get bullet from previous entry if it exists
				if bullet.is_none() {
					bullet = document
						.last_element::<ListEntry>()
						.and_then(|prev| prev.bullet.clone())
				}

				// Depth
				let depth = ListRule::parse_depth(
					captures.get(1).unwrap().as_str(),
					document,
					offset.unwrap_or(0),
				);

				// Content
				let entry_start = captures.get(0).unwrap().start();
				let mut entry_content = captures.get(3).unwrap().as_str().to_string();
				let mut spacing: Option<(Range<usize>, &str)> = None;
				while let Some(captures) = self.continue_re.captures_at(content, end_cursor.pos) {
					// Break if next element is another entry
					if captures.get(0).unwrap().start() != end_cursor.pos
						|| captures
							.get(2)
							.unwrap()
							.as_str()
							.find(|c| c == '*' || c == '-')
							== Some(0)
					{
						break;
					}
					// Advance cursor
					end_cursor = end_cursor.at(captures.get(0).unwrap().end());

					// Spacing
					let current_spacing = captures.get(1).unwrap().as_str();
					if let Some(spacing) = &spacing {
						if spacing.1 != current_spacing {
							reports.push(
								Report::build(
									ReportKind::Warning,
									cursor.source.clone(),
									captures.get(1).unwrap().start(),
								)
								.with_message("Invalid list entry spacing")
								.with_label(
									Label::new((
										cursor.source.clone(),
										captures.get(1).unwrap().range(),
									))
									.with_message("Spacing for list entries do not match")
									.with_color(parser.colors().warning),
								)
								.with_label(
									Label::new((cursor.source.clone(), spacing.0.clone()))
										.with_message("Previous spacing")
										.with_color(parser.colors().warning),
								)
								.finish(),
							);
						}
					} else {
						spacing = Some((captures.get(1).unwrap().range(), current_spacing));
					}

					entry_content += " ";
					entry_content += captures.get(2).unwrap().as_str();
				}

				// Parse entry content
				let token = Token::new(entry_start..end_cursor.pos, end_cursor.source.clone());
				let entry_src = Rc::new(VirtualSource::new(
					token.clone(),
					"List Entry".to_string(),
					entry_content,
				));
				let parsed_content = match util::parse_paragraph(parser, entry_src, document) {
					Err(err) => {
						reports.push(
							Report::build(ReportKind::Warning, token.source(), token.range.start)
								.with_message("Unable to Parse List Entry")
								.with_label(
									Label::new((token.source(), token.range.clone()))
										.with_message(err)
										.with_color(parser.colors().warning),
								)
								.finish(),
						);
						break;
					}
					Ok(mut paragraph) => std::mem::replace(&mut paragraph.content, vec![]),
				};

				if let Some(previous_depth) = document
					.last_element::<ListEntry>()
					.map(|ent| ent.numbering.clone())
				{
					ListRule::push_markers(&token, parser, document, &previous_depth, &depth);
				} else {
					ListRule::push_markers(&token, parser, document, &vec![], &depth);
				}

				parser.push(
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
		ListRule::push_markers(&token, parser, document, &current, &Vec::new());

		(end_cursor, reports)
	}

	// TODO
	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Option<Vec<(String, Function<'lua>)>> { None }
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::elements::paragraph::Paragraph;
	use crate::elements::text::Text;
	use crate::parser::langparser::LangParser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;

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
		let doc = parser.parse(source, None);

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
}
