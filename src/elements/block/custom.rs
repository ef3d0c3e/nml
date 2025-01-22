use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use core::fmt;
use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;

use compiler::compiler::CompilerOutput;
use runtime_format::FormatArgs;
use runtime_format::FormatKey;
use runtime_format::FormatKeyError;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target::HTML;
use crate::compiler::sanitize::Sanitizer;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::parser::parser::ParserState;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::reports::Report;
use crate::parser::source::Token;

use super::data::BlockType;
use super::elem::Block;
use super::style::AuthorPos;
use super::style::QuoteStyle;

#[derive(Debug)]
struct QuoteData {
	pub(self) author: Option<String>,
	pub(self) cite: Option<String>,
	pub(self) url: Option<String>,
	pub(self) style: Rc<QuoteStyle>,
}

#[derive(Debug)]
#[auto_registry::auto_registry(registry = "block_types")]
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

struct QuoteFmtPair<'a>(Sanitizer, &'a QuoteData);

impl FormatKey for QuoteFmtPair<'_> {
	fn fmt(&self, key: &str, f: &mut fmt::Formatter<'_>) -> Result<(), FormatKeyError> {
		match key {
			"author" => write!(
				f,
				"{}",
				self.0
					.sanitize(self.1.author.as_ref().unwrap_or(&"".into()))
			)
			.map_err(FormatKeyError::Fmt),
			"cite" => write!(
				f,
				"{}",
				self.0.sanitize(self.1.cite.as_ref().unwrap_or(&"".into()))
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
			.current(QuoteStyle::key())
			.downcast_rc::<QuoteStyle>()
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

	fn compile<'e>(
		&'e self,
		block: &'e Block,
		properties: &'e Box<dyn Any>,
		compiler: &'e Compiler,
		document: &'e dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		let quote = properties.downcast_ref::<QuoteData>().unwrap();

		match compiler.target() {
			HTML => {
				output.add_content(r#"<div class="blockquote-content">"#);
				let format_author = || -> Result<String, Vec<Report>> {
					let mut result = String::new();

					if quote.cite.is_some() || quote.author.is_some() {
						result += r#"<p class="blockquote-author">"#;
						let fmt_pair = QuoteFmtPair(compiler.sanitizer(), quote);
						let format_string = match (quote.author.is_some(), quote.cite.is_some()) {
							(true, true) => {
								compiler.sanitize_format(quote.style.format[0].as_str())
							}
							(true, false) => {
								compiler.sanitize_format(quote.style.format[1].as_str())
							}
							(false, false) => {
								compiler.sanitize_format(quote.style.format[2].as_str())
							}
							_ => panic!(""),
						};
						let args = FormatArgs::new(format_string.as_str(), &fmt_pair);
						args.status().map_err(|err| vec![
								make_err!(block.location.source(), "Invalid Blockquote".into(),
								span(block.location.range.clone(), format!("Failed to format Blockquote style `{format_string}`: {err}")))] );

						result += args.to_string().as_str();
						result += "</p>";
					}
					Ok(result)
				};

				if let Some(url) = &quote.url {
					output.add_content(
						format!(r#"<blockquote cite="{}">"#, compiler.sanitize(url)).as_str(),
					);
				} else {
					output.add_content("<blockquote>");
				}
				if quote.style.author_pos == AuthorPos::Before {
					output.add_content(format_author()?.as_str());
				}

				let mut in_paragraph = false;
				for elem in &block.content {
					if elem.kind() == ElemKind::Block {
						if in_paragraph {
							output.add_content("</p>");
							in_paragraph = false;
						}
						elem.compile(compiler, document, output)?;
					} else {
						if !in_paragraph {
							output.add_content("<p>");
							in_paragraph = true;
						}
						elem.compile(compiler, document, output)?;
					}
				}
				if in_paragraph {
					output.add_content("</p>");
				}
				output.add_content("</blockquote>");
				if quote.style.author_pos == AuthorPos::After {
					output.add_content(format_author()?);
				}

				output.add_content("</div>");
			}
			_ => todo!(""),
		}
		Ok(())
	}
}

#[derive(Debug, Default)]
#[auto_registry::auto_registry(registry = "block_types")]
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

	fn compile<'e>(
		&'e self,
		block: &'e Block,
		_properties: &'e Box<dyn Any>,
		compiler: &'e Compiler,
		document: &'e dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				output.add_content(r#"<div class="block-warning">"#);
				for elem in &block.content {
					elem.compile(compiler, document, output)?;
				}
				output.add_content("</div>");
			}
			_ => todo!(""),
		}
		Ok(())
	}
}

#[derive(Debug, Default)]
#[auto_registry::auto_registry(registry = "block_types")]
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

	fn compile<'e>(
		&'e self,
		block: &'e Block,
		_properties: &'e Box<dyn Any>,
		compiler: &'e Compiler,
		document: &'e dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				output.add_content(r#"<div class="block-note">"#);
				for elem in &block.content {
					elem.compile(compiler, document, output)?;
				}
				output.add_content("</div>");
			}
			_ => todo!(""),
		}
		Ok(())
	}
}

#[derive(Debug, Default)]
#[auto_registry::auto_registry(registry = "block_types")]
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

	fn compile<'e>(
		&'e self,
		block: &'e Block,
		_properties: &'e Box<dyn Any>,
		compiler: &'e Compiler,
		document: &'e dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				output.add_content(r#"<div class="block-todo">"#);
				for elem in &block.content {
					elem.compile(compiler, document, output)?;
				}
				output.add_content("</div>");
			}
			_ => todo!(""),
		}
		Ok(())
	}
}

#[derive(Debug, Default)]
#[auto_registry::auto_registry(registry = "block_types")]
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

	fn compile<'e>(
		&'e self,
		block: &'e Block,
		_properties: &'e Box<dyn Any>,
		compiler: &'e Compiler,
		document: &'e dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				output.add_content(r#"<div class="block-tip">"#);
				for elem in &block.content {
					elem.compile(compiler, document, output)?;
				}
				output.add_content("</div>");
			}
			_ => todo!(""),
		}
		Ok(())
	}
}

#[derive(Debug, Default)]
#[auto_registry::auto_registry(registry = "block_types")]
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

	fn compile<'e>(
		&'e self,
		block: &'e Block,
		_properties: &'e Box<dyn Any>,
		compiler: &'e Compiler,
		document: &'e dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				output.add_content(r#"<div class="block-caution">"#);
				for elem in &block.content {
					elem.compile(compiler, document, output)?;
				}
				output.add_content("</div>");
			}
			_ => todo!(""),
		}
		Ok(())
	}
}
