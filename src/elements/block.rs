use std::any::Any;
use std::rc::Rc;

use ariadne::Fmt;
use document::element::ContainerElement;
use elements::list::ListEntry;
use elements::list::ListMarker;
use elements::paragraph::Paragraph;
use elements::text::Text;
use lsp::conceal::Conceals;
use lsp::semantic::Semantics;
use parser::parser::SharedState;
use parser::source::VirtualSource;
use regex::Regex;
use serde_json::json;

use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::block::BlockType;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::reports::macros::*;
use crate::parser::reports::Report;
use crate::parser::reports::*;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::source::Token;
use crate::parser::util::escape_source;

/// Defines the default blocks
mod default_blocks {
	use core::fmt;
	use std::collections::HashMap;

	use compiler::compiler::Target::HTML;
	use parser::property::Property;
	use parser::property::PropertyParser;
	use runtime_format::FormatArgs;
	use runtime_format::FormatKey;
	use runtime_format::FormatKeyError;

	use super::*;
	use crate::compiler::compiler::Compiler;
	use crate::document::document::Document;
	use crate::parser::parser::ParserState;
	use crate::parser::reports::Report;

	#[derive(Debug)]
	struct QuoteData {
		pub(self) author: Option<String>,
		pub(self) cite: Option<String>,
		pub(self) url: Option<String>,
		pub(self) style: Rc<block_style::QuoteStyle>,
	}

	#[derive(Debug)]
	pub struct Quote {
		properties: PropertyParser,
	}

	impl Default for Quote {
		fn default() -> Self {
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
				properties: PropertyParser { properties: props },
			}
		}
	}

	struct QuoteFmtPair<'a>(crate::compiler::compiler::Target, &'a QuoteData);

	impl FormatKey for QuoteFmtPair<'_> {
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

	impl BlockType for Quote {
		fn name(&self) -> &'static str { "Quote" }

		fn parse_properties(
			&self,
			reports: &mut Vec<Report>,
			state: &ParserState,
			token: Token,
		) -> Option<Box<dyn Any>> {
			// Get style
			let style = state
				.shared
				.styles
				.borrow()
				.current(block_style::STYLE_KEY_QUOTE)
				.downcast_rc::<block_style::QuoteStyle>()
				.unwrap();

			// Parse properties
			let properties = match self.properties.parse("Block Quote", reports, state, token) {
				Some(props) => props,
				None => return None,
			};
			match (
				properties.get_opt(reports, "author", |_, value| {
					Result::<_, String>::Ok(value.value.clone())
				}),
				properties.get_opt(reports, "cite", |_, value| {
					Result::<_, String>::Ok(value.value.clone())
				}),
				properties.get_opt(reports, "url", |_, value| {
					Result::<_, String>::Ok(value.value.clone())
				}),
			) {
				(Some(author), Some(cite), Some(url)) => Some(Box::new(QuoteData {
					author,
					cite,
					url,
					style,
				})),
				_ => None,
			}
		}

		fn compile(
			&self,
			block: &Block,
			properties: &Box<dyn Any>,
			compiler: &Compiler,
			document: &dyn Document,
			cursor: usize,
		) -> Result<String, String> {
			let quote = properties.downcast_ref::<QuoteData>().unwrap();

			match compiler.target() {
				HTML => {
					let mut result = r#"<div class="blockquote-content">"#.to_string();
					let format_author = || -> Result<String, String> {
						let mut result = String::new();

						if quote.cite.is_some() || quote.author.is_some() {
							result += r#"<p class="blockquote-author">"#;
							let fmt_pair = QuoteFmtPair(compiler.target(), quote);
							let format_string = match (quote.author.is_some(), quote.cite.is_some())
							{
								(true, true) => Compiler::sanitize_format(
									fmt_pair.0,
									quote.style.format[0].as_str(),
								),
								(true, false) => Compiler::sanitize_format(
									fmt_pair.0,
									quote.style.format[1].as_str(),
								),
								(false, false) => Compiler::sanitize_format(
									fmt_pair.0,
									quote.style.format[2].as_str(),
								),
								_ => panic!(""),
							};
							let args = FormatArgs::new(format_string.as_str(), &fmt_pair);
							args.status().map_err(|err| {
								format!(
									"Failed to format Blockquote style `{format_string}`: {err}"
								)
							})?;
							result += args.to_string().as_str();
							result += "</p>";
						}
						Ok(result)
					};

					if let Some(url) = &quote.url {
						result +=
							format!(r#"<blockquote cite="{}">"#, Compiler::sanitize(HTML, url))
								.as_str();
					} else {
						result += "<blockquote>";
					}
					if quote.style.author_pos == block_style::AuthorPos::Before {
						result += format_author()?.as_str();
					}

					let mut in_paragraph = false;
					for elem in &block.content {
						if elem.kind() == ElemKind::Block {
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
					if quote.style.author_pos == block_style::AuthorPos::After {
						result += format_author().map_err(|err| err.to_string())?.as_str();
					}

					result += "</div>";
					Ok(result)
				}
				_ => todo!(""),
			}
		}
	}

	#[derive(Debug, Default)]
	pub struct Warning;

	impl BlockType for Warning {
		fn name(&self) -> &'static str { "Warning" }

		fn parse_properties(
			&self,
			_reports: &mut Vec<Report>,
			_state: &ParserState,
			_token: Token,
		) -> Option<Box<dyn Any>> {
			Some(Box::new(()))
		}

		fn compile(
			&self,
			block: &Block,
			_properties: &Box<dyn Any>,
			compiler: &Compiler,
			document: &dyn Document,
			cursor: usize,
		) -> Result<String, String> {
			match compiler.target() {
				HTML => {
					let mut result = r#"<div class="block-warning">"#.to_string();
					for elem in &block.content {
						result += elem
							.compile(compiler, document, cursor + result.len())?
							.as_str();
					}
					result += "</div>";
					Ok(result)
				}
				_ => todo!(""),
			}
		}
	}

	#[derive(Debug, Default)]
	pub struct Note;

	impl BlockType for Note {
		fn name(&self) -> &'static str { "Note" }

		fn parse_properties(
			&self,
			_reports: &mut Vec<Report>,
			_state: &ParserState,
			_token: Token,
		) -> Option<Box<dyn Any>> {
			Some(Box::new(()))
		}

		fn compile(
			&self,
			block: &Block,
			_properties: &Box<dyn Any>,
			compiler: &Compiler,
			document: &dyn Document,
			cursor: usize,
		) -> Result<String, String> {
			match compiler.target() {
				HTML => {
					let mut result = r#"<div class="block-note">"#.to_string();
					for elem in &block.content {
						result += elem
							.compile(compiler, document, cursor + result.len())?
							.as_str();
					}
					result += "</div>";
					Ok(result)
				}
				_ => todo!(""),
			}
		}
	}

	#[derive(Debug, Default)]
	pub struct Todo;

	impl BlockType for Todo {
		fn name(&self) -> &'static str { "Todo" }

		fn parse_properties(
			&self,
			_reports: &mut Vec<Report>,
			_state: &ParserState,
			_token: Token,
		) -> Option<Box<dyn Any>> {
			Some(Box::new(()))
		}

		fn compile(
			&self,
			block: &Block,
			_properties: &Box<dyn Any>,
			compiler: &Compiler,
			document: &dyn Document,
			cursor: usize,
		) -> Result<String, String> {
			match compiler.target() {
				HTML => {
					let mut result = r#"<div class="block-todo">"#.to_string();
					for elem in &block.content {
						result += elem
							.compile(compiler, document, cursor + result.len())?
							.as_str();
					}
					result += "</div>";
					Ok(result)
				}
				_ => todo!(""),
			}
		}
	}

	#[derive(Debug, Default)]
	pub struct Tip;

	impl BlockType for Tip {
		fn name(&self) -> &'static str { "Tip" }

		fn parse_properties(
			&self,
			_reports: &mut Vec<Report>,
			_state: &ParserState,
			_token: Token,
		) -> Option<Box<dyn Any>> {
			Some(Box::new(()))
		}

		fn compile(
			&self,
			block: &Block,
			_properties: &Box<dyn Any>,
			compiler: &Compiler,
			document: &dyn Document,
			cursor: usize,
		) -> Result<String, String> {
			match compiler.target() {
				HTML => {
					let mut result = r#"<div class="block-tip">"#.to_string();
					for elem in &block.content {
						result += elem
							.compile(compiler, document, cursor + result.len())?
							.as_str();
					}
					result += "</div>";
					Ok(result)
				}
				_ => todo!(""),
			}
		}
	}

	#[derive(Debug, Default)]
	pub struct Caution;

	impl BlockType for Caution {
		fn name(&self) -> &'static str { "Caution" }

		fn parse_properties(
			&self,
			_reports: &mut Vec<Report>,
			_state: &ParserState,
			_token: Token,
		) -> Option<Box<dyn Any>> {
			Some(Box::new(()))
		}

		fn compile(
			&self,
			block: &Block,
			_properties: &Box<dyn Any>,
			compiler: &Compiler,
			document: &dyn Document,
			cursor: usize,
		) -> Result<String, String> {
			match compiler.target() {
				HTML => {
					let mut result = r#"<div class="block-caution">"#.to_string();
					for elem in &block.content {
						result += elem
							.compile(compiler, document, cursor + result.len())?
							.as_str();
					}
					result += "</div>";
					Ok(result)
				}
				_ => todo!(""),
			}
		}
	}
}

#[derive(Debug)]
pub struct Block {
	pub(self) location: Token,
	pub(self) content: Vec<Box<dyn Element>>,
	pub(self) block_type: Rc<dyn BlockType>,
	pub(self) block_properties: Box<dyn Any>,
}

impl Element for Block {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Block }
	fn element_name(&self) -> &'static str { "Block" }
	fn compile(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		cursor: usize,
	) -> Result<String, String> {
		self.block_type
			.compile(self, &self.block_properties, compiler, document, cursor)
	}

	fn as_container(&self) -> Option<&dyn ContainerElement> { Some(self) }
}

impl ContainerElement for Block {
	fn contained(&self) -> &Vec<Box<dyn Element>> { &self.content }

	fn push(&mut self, elem: Box<dyn Element>) -> Result<(), String> {
		self.content.push(elem);
		Ok(())
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::block")]
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
		holder.insert(Rc::new(default_blocks::Quote::default()));
		holder.insert(Rc::new(default_blocks::Warning::default()));
		holder.insert(Rc::new(default_blocks::Note::default()));
		holder.insert(Rc::new(default_blocks::Todo::default()));
		holder.insert(Rc::new(default_blocks::Tip::default()));
		holder.insert(Rc::new(default_blocks::Caution::default()));

		let mut holder = state.styles.borrow_mut();
		holder.set_current(Rc::new(block_style::QuoteStyle::default()));
	}
}

mod block_style {
	use serde::Deserialize;
	use serde::Serialize;

	use crate::impl_elementstyle;

	pub static STYLE_KEY_QUOTE: &str = "style.block.quote";

	#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
	pub enum AuthorPos {
		Before,
		After,
		None,
	}

	#[derive(Debug, Serialize, Deserialize)]
	pub struct QuoteStyle {
		pub author_pos: AuthorPos,
		pub format: [String; 3],
	}

	impl Default for QuoteStyle {
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

	impl_elementstyle!(QuoteStyle, STYLE_KEY_QUOTE);
}

#[cfg(test)]
mod tests {
	use block_style::QuoteStyle;

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
>[!Quote][author=A, cite=B, url=C]
>Some entry
>contin**ued here
>**
AFTER
>    [!Quote]
> Another
>
> quote
>>[!Quote][author=B]
>>Nested
>>> [!Quote]
>>> More nested
AFTER
>[!Warning]
>>[!Note][]
>>>[!Todo]
>>>>[!Tip][]
>>>>>[!Caution]
>>>>>Nested
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
			Block {
				Text { content == "Some entry contin" };
				Style;
				Text { content == "ued here" };
				Style;
			};
			Paragraph { Text{ content == "AFTER" }; };
			Block {
				Text { content == "Another" };
				Text { content == " " };
				Text { content == "quote" };
				Block {
					Text { content == "Nested" };
					Block {
						Text { content == "More nested" };
					};
				};
			};
			Paragraph { Text{ content == "AFTER" }; };
			Block {
				Block {
					Block {
						Block {
							Block {
								Text { content == "Nested" };
							};
						};
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
@@style.block.quote = {
	"author_pos": "Before",
	"format": ["{cite} by {author}", "Author: {author}", "From: {cite}"]
}
PRE
>[!Quote][author=A, cite=B, url=C]
>Some entry
>contin**ued here
>**
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
			.current(block_style::STYLE_KEY_QUOTE)
			.downcast_rc::<QuoteStyle>()
			.unwrap();

		assert_eq!(style.author_pos, block_style::AuthorPos::Before);
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
