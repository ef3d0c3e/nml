use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::parser::Parser;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use crate::parser::util;
use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use mlua::Function;
use mlua::Lua;
use regex::Captures;
use regex::Regex;
use std::ops::Range;
use std::rc::Rc;

#[derive(Debug)]
pub struct Link {
	pub location: Token,
	/// Display content of link
	pub display: Vec<Box<dyn Element>>,
	/// Url of link
	pub url: String,
}

impl Element for Link {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Inline }
	fn element_name(&self) -> &'static str { "Link" }
	fn compile(&self, compiler: &Compiler, document: &dyn Document) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				let mut result = format!(
					"<a href=\"{}\">",
					Compiler::sanitize(compiler.target(), self.url.as_str())
				);

				for elem in &self.display {
					result += elem.compile(compiler, document)?.as_str();
				}

				result += "</a>";
				Ok(result)
			}
			_ => todo!(""),
		}
	}

	fn as_container(&self) -> Option<&dyn ContainerElement> { Some(self) }
}

impl ContainerElement for Link {
	fn contained(&self) -> &Vec<Box<dyn Element>> { &self.display }

	fn push(&mut self, elem: Box<dyn Element>) -> Result<(), String> {
		if elem.downcast_ref::<Link>().is_some() {
			return Err("Tried to push a link inside of a link".to_string());
		}
		self.display.push(elem);
		Ok(())
	}
}

pub struct LinkRule {
	re: [Regex; 1],
}

impl LinkRule {
	pub fn new() -> Self {
		Self {
			re: [Regex::new(r"\[((?:\\.|[^\\\\])*?)\]\(((?:\\.|[^\\\\])*?)\)").unwrap()],
		}
	}
}

impl RegexRule for LinkRule {
	fn name(&self) -> &'static str { "Link" }

	fn regexes(&self) -> &[Regex] { &self.re }

	fn on_regex_match<'a>(
		&self,
		_: usize,
		parser: &dyn Parser,
		document: &'a (dyn Document<'a> + 'a),
		token: Token,
		matches: Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let link_display = match matches.get(1) {
			Some(display) => {
				if display.as_str().is_empty() {
					reports.push(
						Report::build(ReportKind::Error, token.source(), display.start())
							.with_message("Empty link name")
							.with_label(
								Label::new((token.source().clone(), display.range()))
									.with_message("Link name is empty")
									.with_color(parser.colors().error),
							)
							.finish(),
					);
					return reports;
				}
				let processed = util::process_escaped('\\', "]", display.as_str());
				if processed.is_empty() {
					reports.push(
						Report::build(ReportKind::Error, token.source(), display.start())
							.with_message("Empty link name")
							.with_label(
								Label::new((token.source(), display.range()))
									.with_message(format!(
										"Link name is empty. Once processed, `{}` yields `{}`",
										display.as_str().fg(parser.colors().highlight),
										processed.fg(parser.colors().highlight),
									))
									.with_color(parser.colors().error),
							)
							.finish(),
					);
					return reports;
				}

				let source = Rc::new(VirtualSource::new(
					Token::new(display.range(), token.source()),
					"Link Display".to_string(),
					processed,
				));
				match util::parse_paragraph(parser, source, document) {
					Err(err) => {
						reports.push(
							Report::build(ReportKind::Error, token.source(), display.start())
								.with_message("Failed to parse link display")
								.with_label(
									Label::new((token.source(), display.range()))
										.with_message(err.to_string())
										.with_color(parser.colors().error),
								)
								.finish(),
						);
						return reports;
					}
					Ok(mut paragraph) => std::mem::replace(&mut paragraph.content, vec![]),
				}
			}
			_ => panic!("Empty link name"),
		};

		let link_url = match matches.get(2) {
			Some(url) => {
				if url.as_str().is_empty() {
					reports.push(
						Report::build(ReportKind::Error, token.source(), url.start())
							.with_message("Empty link url")
							.with_label(
								Label::new((token.source(), url.range()))
									.with_message("Link url is empty")
									.with_color(parser.colors().error),
							)
							.finish(),
					);
					return reports;
				}
				let text_content = util::process_text(document, url.as_str());

				if text_content.as_str().is_empty() {
					reports.push(
						Report::build(ReportKind::Error, token.source(), url.start())
							.with_message("Empty link url")
							.with_label(
								Label::new((token.source(), url.range()))
									.with_message(format!(
										"Link url is empty. Once processed, `{}` yields `{}`",
										url.as_str().fg(parser.colors().highlight),
										text_content.as_str().fg(parser.colors().highlight),
									))
									.with_color(parser.colors().error),
							)
							.finish(),
					);
					return reports;
				}
				text_content
			}
			_ => panic!("Empty link url"),
		};

		parser.push(
			document,
			Box::new(Link {
				location: token,
				display: link_display,
				url: link_url,
			}),
		);

		return reports;
	}

	// TODO
	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Option<Vec<(String, Function<'lua>)>> { None }
}

#[cfg(test)]
mod tests {
	use crate::elements::paragraph::Paragraph;
	use crate::elements::style::Style;
	use crate::elements::text::Text;
	use crate::parser::langparser::LangParser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;

	use super::*;

	#[test]
	fn parser() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
Some [link](url).
[**BOLD link**](another url)
			"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let doc = parser.parse(source, None);

		validate_document!(doc.content().borrow(), 0,
			Paragraph {
				Text { content == "Some " };
				Link { url == "url" } { Text { content == "link" }; };
				Text { content == "." };
				Link { url == "another url" } {
					Style;
					Text { content == "BOLD link" };
					Style;
				};
			};
		);
	}
}
