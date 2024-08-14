use core::fmt;
use std::any::Any;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use blockquote_style::AuthorPos::After;
use blockquote_style::AuthorPos::Before;
use blockquote_style::BlockquoteStyle;
use regex::Match;
use regex::Regex;
use runtime_format::FormatArgs;
use runtime_format::FormatError;
use runtime_format::FormatKey;
use runtime_format::FormatKeyError;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::parser::ParserState;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use crate::parser::style::StyleHolder;
use crate::parser::util::parse_paragraph;
use crate::parser::util::process_escaped;
use crate::parser::util::Property;
use crate::parser::util::PropertyParser;

#[derive(Debug)]
pub struct Blockquote {
	pub(self) location: Token,
	pub(self) content: Vec<Box<dyn Element>>,
	pub(self) author: Option<String>,
	pub(self) cite: Option<String>,
	pub(self) url: Option<String>,
	/// Style of the blockquote
	pub(self) style: Rc<blockquote_style::BlockquoteStyle>,
}

struct FmtPair<'a>(Target, &'a Blockquote);

impl FormatKey for FmtPair<'_> {
	fn fmt(&self, key: &str, f: &mut fmt::Formatter<'_>) -> Result<(), FormatKeyError> {
		match key {
			"author" => write!(
				f,
				"{}",
				Compiler::sanitize(self.0, self.1.author.as_ref().unwrap_or(&"".into()))
			)
			.map_err(FormatKeyError::Fmt),
			"cite" => write!(
				f,
				"{}",
				Compiler::sanitize(self.0, self.1.cite.as_ref().unwrap_or(&"".into()))
			)
			.map_err(FormatKeyError::Fmt),
			_ => Err(FormatKeyError::UnknownKey),
		}
	}
}

impl Element for Blockquote {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Block }

	fn element_name(&self) -> &'static str { "Blockquote" }

	fn compile(&self, compiler: &Compiler, document: &dyn Document, cursor: usize) -> Result<String, String> {
		match compiler.target() {
			HTML => {
				let mut result = r#"<div class="blockquote-content">"#.to_string();
				let format_author = || -> Result<String, FormatError> {
					let mut result = String::new();

					if self.cite.is_some() || self.author.is_some() {
						result += r#"<p class="blockquote-author">"#;
						let fmt_pair = FmtPair(compiler.target(), self);
						match (self.author.is_some(), self.cite.is_some()) {
							(true, true) => {
								let args =
									FormatArgs::new(self.style.format[0].as_str(), &fmt_pair);
								args.status()?;
								result += args.to_string().as_str();
							}
							(true, false) => {
								let args =
									FormatArgs::new(self.style.format[1].as_str(), &fmt_pair);
								args.status()?;
								result += args.to_string().as_str();
							}
							(false, false) => {
								let args =
									FormatArgs::new(self.style.format[2].as_str(), &fmt_pair);
								args.status()?;
								result += args.to_string().as_str();
							}
							_ => panic!(""),
						}
						result += "</p>";
					}
					Ok(result)
				};

				if let Some(url) = &self.url {
					result += format!(r#"<blockquote cite="{}">"#, Compiler::sanitize(HTML, url))
						.as_str();
				} else {
					result += "<blockquote>";
				}
				if self.style.author_pos == Before {
					result += format_author().map_err(|err| err.to_string())?.as_str();
				}

				result += "<p>";
				for elem in &self.content {
					result += elem.compile(compiler, document, cursor+result.len())?.as_str();
				}
				result += "</p></blockquote>";
				if self.style.author_pos == After {
					result += format_author().map_err(|err| err.to_string())?.as_str();
				}

				result += "</div>";
				Ok(result)
			}
			_ => todo!(""),
		}
	}

	fn as_container(&self) -> Option<&dyn ContainerElement> { Some(self) }
}

impl ContainerElement for Blockquote {
	fn contained(&self) -> &Vec<Box<dyn Element>> { &self.content }

	fn push(&mut self, elem: Box<dyn Element>) -> Result<(), String> {
		if elem.kind() == ElemKind::Block {
			return Err("Cannot add block element inside a blockquote".to_string());
		}

		self.content.push(elem);
		Ok(())
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::blockquote")]
pub struct BlockquoteRule {
	start_re: Regex,
	continue_re: Regex,
	properties: PropertyParser,
}

impl BlockquoteRule {
	pub fn new() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"author".to_string(),
			Property::new(false, "Quote author".to_string(), None),
		);
		props.insert(
			"cite".to_string(),
			Property::new(false, "Quote source".to_string(), None),
		);
		props.insert(
			"url".to_string(),
			Property::new(false, "Quote source url".to_string(), None),
		);

		Self {
			start_re: Regex::new(r"(?:^|\n)>(?:\[((?:\\.|[^\\\\])*?)\])?\s*(.*)").unwrap(),
			continue_re: Regex::new(r"(?:^|\n)>(\s*)(.*)").unwrap(),
			properties: PropertyParser { properties: props },
		}
	}

	fn parse_properties(
		&self,
		m: Match,
	) -> Result<(Option<String>, Option<String>, Option<String>), String> {
		let processed = process_escaped('\\', "]", m.as_str());
		let pm = self.properties.parse(processed.as_str())?;

		let author = pm
			.get("author", |_, s| -> Result<String, ()> { Ok(s.to_string()) })
			.map(|(_, s)| s)
			.ok();
		let cite = pm
			.get("cite", |_, s| -> Result<String, ()> { Ok(s.to_string()) })
			.map(|(_, s)| s)
			.ok();
		let url = pm
			.get("url", |_, s| -> Result<String, ()> { Ok(s.to_string()) })
			.map(|(_, s)| s)
			.ok();

		Ok((author, cite, url))
	}
}

impl Rule for BlockquoteRule {
	fn name(&self) -> &'static str { "Blockquote" }

	fn previous(&self) -> Option<&'static str> { Some("List") }

	fn next_match(&self, _state: &ParserState, cursor: &Cursor) -> Option<(usize, Box<dyn Any>)> {
		self.start_re
			.find_at(cursor.source.content(), cursor.pos).map(|m| (m.start(), Box::new([false; 0]) as Box<dyn Any>))
	}

	fn on_match<'a>(
		&self,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
		cursor: Cursor,
		_match_data: Box<dyn Any>,
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
				let mut author = None;
				let mut cite = None;
				let mut url = None;
				if let Some(properties) = captures.get(1) {
					match self.parse_properties(properties) {
						Err(err) => {
							reports.push(
								Report::build(
									ReportKind::Warning,
									cursor.source.clone(),
									properties.start(),
								)
								.with_message("Invalid Blockquote Properties")
								.with_label(
									Label::new((cursor.source.clone(), properties.range()))
										.with_message(err)
										.with_color(state.parser.colors().warning),
								)
								.finish(),
							);
							break;
						}
						Ok(props) => (author, cite, url) = props,
					}
				}

				// Content
				let entry_start = captures.get(0).unwrap().start();
				let mut entry_content = captures.get(2).unwrap().as_str().to_string();
				let mut spacing: Option<(Range<usize>, &str)> = None;
				while let Some(captures) = self.continue_re.captures_at(content, end_cursor.pos) {
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
								.with_message("Invalid Blockquote Spacing")
								.with_label(
									Label::new((
										cursor.source.clone(),
										captures.get(1).unwrap().range(),
									))
									.with_message("Spacing for blockquote entries do not match")
									.with_color(state.parser.colors().warning),
								)
								.with_label(
									Label::new((cursor.source.clone(), spacing.0.clone()))
										.with_message("Previous spacing")
										.with_color(state.parser.colors().warning),
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
					"Blockquote Entry".to_string(),
					entry_content,
				));
				let parsed_content = match parse_paragraph(state, entry_src, document) {
					Err(err) => {
						reports.push(
							Report::build(ReportKind::Warning, token.source(), token.range.start)
								.with_message("Unable to Parse Blockquote Entry")
								.with_label(
									Label::new((token.source(), token.range.clone()))
										.with_message(err)
										.with_color(state.parser.colors().warning),
								)
								.finish(),
						);
						break;
					}
					Ok(mut paragraph) => std::mem::take(&mut paragraph.content),
				};

				// Get style
				let style = state
					.shared
					.styles
					.borrow()
					.current(blockquote_style::STYLE_KEY)
					.downcast_rc::<BlockquoteStyle>()
					.unwrap();

				state.push(
					document,
					Box::new(Blockquote {
						location: Token::new(
							entry_start..end_cursor.pos,
							end_cursor.source.clone(),
						),
						content: parsed_content,
						author,
						cite,
						url,
						style,
					}),
				);
			} else {
				break;
			}
		}

		(end_cursor, reports)
	}

	fn register_styles(&self, holder: &mut StyleHolder) {
		holder.set_current(Rc::new(BlockquoteStyle::default()));
	}
}

mod blockquote_style {
	use serde::Deserialize;
	use serde::Serialize;

	use crate::impl_elementstyle;

	pub static STYLE_KEY: &str = "style.blockquote";

	#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
	pub enum AuthorPos {
		Before,
		After,
		None,
	}

	#[derive(Debug, Serialize, Deserialize)]
	pub struct BlockquoteStyle {
		pub author_pos: AuthorPos,
		pub format: [String; 3],
	}

	impl Default for BlockquoteStyle {
		fn default() -> Self {
			Self {
				author_pos: AuthorPos::After,
				format: [
					"{author}, {cite}".into(),
					"{author}".into(),
					"{cite}".into(),
				],
			}
		}
	}

	impl_elementstyle!(BlockquoteStyle, STYLE_KEY);
}

#[cfg(test)]
mod tests {
	use crate::elements::paragraph::Paragraph;
	use crate::elements::style::Style;
	use crate::elements::text::Text;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;

	use super::*;

	#[test]
	pub fn parser() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
BEFORE
>[author=A, cite=B, url=C] Some entry
> contin**ued here
> **
AFTER
"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

		validate_document!(doc.content().borrow(), 0,
			Paragraph { Text{ content == "BEFORE" }; };
			Blockquote {
				author == Some("A".to_string()),
				cite == Some("B".to_string()),
				url == Some("C".to_string())
			} {
				Text { content == "Some entry contin" };
				Style;
				Text { content == "ued here " };
				Style;
			};
			Paragraph { Text{ content == "AFTER" }; };
		);
	}

	#[test]
	pub fn style() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
@@style.blockquote = {
	"author_pos": "Before",
	"format": ["{cite} by {author}", "Author: {author}", "From: {cite}"]
}
PRE
>[author=A, cite=B, url=C] Some entry
> contin**ued here
> **
AFTER
"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (_, state) = parser.parse(ParserState::new(&parser, None), source, None);

		let style = state
			.shared
			.styles
			.borrow()
			.current(blockquote_style::STYLE_KEY)
			.downcast_rc::<BlockquoteStyle>()
			.unwrap();

		assert_eq!(style.author_pos, Before);
		assert_eq!(
			style.format,
			[
				"{cite} by {author}".to_string(),
				"Author: {author}".to_string(),
				"From: {cite}".to_string()
			]
		);
	}
}
