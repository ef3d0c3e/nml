use core::fmt;
use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;

use blockquote_style::AuthorPos::After;
use blockquote_style::AuthorPos::Before;
use blockquote_style::BlockquoteStyle;
use lsp::semantic::Semantics;
use parser::util::escape_source;
use regex::Regex;
use runtime_format::FormatArgs;
use runtime_format::FormatKey;
use runtime_format::FormatKeyError;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::elements::paragraph::Paragraph;
use crate::elements::text::Text;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use crate::parser::style::StyleHolder;

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

	fn compile(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		cursor: usize,
	) -> Result<String, String> {
		match compiler.target() {
			HTML => {
				let mut result = r#"<div class="blockquote-content">"#.to_string();
				let format_author = || -> Result<String, String> {
					let mut result = String::new();

					if self.cite.is_some() || self.author.is_some() {
						result += r#"<p class="blockquote-author">"#;
						let fmt_pair = FmtPair(compiler.target(), self);
						let format_string = match (self.author.is_some(), self.cite.is_some()) {
							(true, true) => {
								Compiler::sanitize_format(fmt_pair.0, self.style.format[0].as_str())
							}
							(true, false) => {
								Compiler::sanitize_format(fmt_pair.0, self.style.format[1].as_str())
							}
							(false, false) => {
								Compiler::sanitize_format(fmt_pair.0, self.style.format[2].as_str())
							}
							_ => panic!(""),
						};
						let args = FormatArgs::new(format_string.as_str(), &fmt_pair);
						args.status().map_err(|err| {
							format!("Failed to format Blockquote style `{format_string}`: {err}")
						})?;
						result += args.to_string().as_str();
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
					result += format_author()?.as_str();
				}

				let mut in_paragraph = false;
				for elem in &self.content {
					if elem.downcast_ref::<Blockquote>().is_some() {
						if in_paragraph {
							result += "</p>";
							in_paragraph = false;
						}
						result += elem
							.compile(compiler, document, cursor + result.len())?
							.as_str();
					} else {
						if !in_paragraph {
							result += "<p>";
							in_paragraph = true;
						}
						result += elem
							.compile(compiler, document, cursor + result.len())?
							.as_str();
					}
				}
				if in_paragraph {
					result += "</p>";
				}
				result += "</blockquote>";
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
			Property::new("Quote author".to_string(), None),
		);
		props.insert(
			"cite".to_string(),
			Property::new("Quote source".to_string(), None),
		);
		props.insert(
			"url".to_string(),
			Property::new("Quote source url".to_string(), None),
		);

		Self {
			start_re: Regex::new(r"(?:^|\n)>(?:\[((?:\\.|[^\\\\])*?)\])?[^\S\r\n]*(.*)").unwrap(),
			continue_re: Regex::new(r"(?:^|\n)>[^\S\r\n]*(.*)").unwrap(),
			properties: PropertyParser { properties: props },
		}
	}
}

impl Rule for BlockquoteRule {
	fn name(&self) -> &'static str { "Blockquote" }

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
		if let Some(captures) = self.start_re.captures_at(content, end_cursor.pos) {
			if captures.get(0).unwrap().start() != end_cursor.pos {
				return (end_cursor, reports);
			}
			// Advance cursor
			end_cursor = end_cursor.at(captures.get(0).unwrap().end());

			// Properties
			let prop_source = escape_source(
				end_cursor.source.clone(),
				captures.get(1).map_or(0..0, |m| m.range()),
				"Blockquote Properties".into(),
				'\\',
				"]",
			);
			let properties =
				match self
					.properties
					.parse("Blockquote", &mut reports, state, prop_source.into())
				{
					Some(props) => props,
					None => return (end_cursor, reports),
				};
			let (author, cite, url) = match (
				properties.get_opt(&mut reports, "author", |_, value| {
					Result::<_, String>::Ok(value.value.clone())
				}),
				properties.get_opt(&mut reports, "cite", |_, value| {
					Result::<_, String>::Ok(value.value.clone())
				}),
				properties.get_opt(&mut reports, "url", |_, value| {
					Result::<_, String>::Ok(value.value.clone())
				}),
			) {
				(Some(author), Some(cite), Some(url)) => (author, cite, url),
				_ => return (end_cursor, reports),
			};

			if let Some((sems, tokens)) =
				Semantics::from_source(cursor.source.clone(), &state.shared.lsp)
			{
				let range = captures.get(0).unwrap().range();
				let start = if content.as_bytes()[range.start] == b'\n' {
					range.start + 1
				} else {
					range.start
				};
				sems.add(start..start + 1, tokens.blockquote_marker);
				if let Some(props) = captures.get(1).map(|m| m.range()) {
					sems.add(props.start - 1..props.start, tokens.blockquote_props_sep);
					sems.add(props.end..props.end + 1, tokens.blockquote_props_sep);
				}
			}

			// Content
			let entry_start = captures.get(2).unwrap().start();
			let mut entry_content = captures.get(2).unwrap().as_str().to_string();
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
					entry_content.len(),
					last + (captures.get(1).unwrap().start() - captures.get(0).unwrap().start() - 1)
						as isize,
				));

				entry_content += "\n";
				entry_content += captures.get(1).unwrap().as_str();

				if let Some((sems, tokens)) =
					Semantics::from_source(cursor.source.clone(), &state.shared.lsp)
				{
					let range = captures.get(0).unwrap().range();
					let start = if content.as_bytes()[range.start] == b'\n' {
						range.start + 1
					} else {
						range.start
					};
					sems.add_to_queue(start..start + 1, tokens.blockquote_marker);
				}
			}

			// Parse entry content
			let token = Token::new(entry_start..end_cursor.pos, end_cursor.source.clone());
			let entry_src = Rc::new(VirtualSource::new_offsets(
				token.clone(),
				"Blockquote Entry".to_string(),
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
				} else if elem.downcast_ref::<Blockquote>().is_some() {
					parsed_content.push(elem);
				} else {
					report_err!(
						&mut reports,
						token.source(),
						"Unable to Parse Blockquote Entry".into(),
						span(
							token.range.clone(),
							"Blockquotes may only contain paragraphs and other blockquotes".into()
						)
					);
					return (end_cursor, reports);
				}
			}

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
					location: Token::new(entry_start..end_cursor.pos, end_cursor.source.clone()),
					content: parsed_content,
					author,
					cite,
					url,
					style,
				}),
			);
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
> Another
>
> quote
>>[author=B] Nested
>>> More nested
END
"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(
			ParserState::new(&parser, None),
			source,
			None,
			ParseMode::default(),
		);

		validate_document!(doc.content().borrow(), 0,
			Paragraph { Text{ content == "BEFORE" }; };
			Blockquote {
				author == Some("A".to_string()),
				cite == Some("B".to_string()),
				url == Some("C".to_string())
			} {
				Text { content == "Some entry contin" };
				Style;
				Text { content == "ued here" };
				Style;
			};
			Paragraph { Text{ content == "AFTER" }; };
			Blockquote {
				Text { content == "Another" };
				Text { content == " " };
				Text { content == "quote" };
				Blockquote { author == Some("B".to_string()) } {
					Text { content == "Nested" };
					Blockquote {
						Text { content == "More nested" };
					};
				};
			};
			Paragraph { Text{ content == "END" }; };
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
		let (_, state) = parser.parse(
			ParserState::new(&parser, None),
			source,
			None,
			ParseMode::default(),
		);

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
