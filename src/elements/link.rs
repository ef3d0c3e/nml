use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::lua::kernel::CTX;
use crate::parser::parser::ParserState;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use crate::parser::util;
use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use regex::Captures;
use regex::Regex;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

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
	fn compile(&self, compiler: &Compiler, document: &dyn Document, cursor: usize) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				let mut result = format!(
					"<a href=\"{}\">",
					Compiler::sanitize(compiler.target(), self.url.as_str())
				);

				for elem in &self.display {
					result += elem.compile(compiler, document, cursor+result.len())?.as_str();
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

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::link")]
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
	fn previous(&self) -> Option<&'static str> { Some("Link") }

	fn regexes(&self) -> &[Regex] { &self.re }

	fn on_regex_match<'a>(
		&self,
		_: usize,
		state: &ParserState,
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
									.with_color(state.parser.colors().error),
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
										display.as_str().fg(state.parser.colors().highlight),
										processed.fg(state.parser.colors().highlight),
									))
									.with_color(state.parser.colors().error),
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
				match util::parse_paragraph(state, source, document) {
					Err(err) => {
						reports.push(
							Report::build(ReportKind::Error, token.source(), display.start())
								.with_message("Failed to parse link display")
								.with_label(
									Label::new((token.source(), display.range()))
										.with_message(err.to_string())
										.with_color(state.parser.colors().error),
								)
								.finish(),
						);
						return reports;
					}
					Ok(mut paragraph) => std::mem::take(&mut paragraph.content),
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
									.with_color(state.parser.colors().error),
							)
							.finish(),
					);
					return reports;
				}
				let text_content = util::process_text(document, url.as_str());

				if text_content.is_empty() {
					reports.push(
						Report::build(ReportKind::Error, token.source(), url.start())
							.with_message("Empty link url")
							.with_label(
								Label::new((token.source(), url.range()))
									.with_message(format!(
										"Link url is empty. Once processed, `{}` yields `{}`",
										url.as_str().fg(state.parser.colors().highlight),
										text_content.as_str().fg(state.parser.colors().highlight),
									))
									.with_color(state.parser.colors().error),
							)
							.finish(),
					);
					return reports;
				}
				text_content
			}
			_ => panic!("Empty link url"),
		};

		state.push(
			document,
			Box::new(Link {
				location: token,
				display: link_display,
				url: link_url,
			}),
		);

		reports
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];

		bindings.push((
			"push".to_string(),
			lua.create_function(|_, (display, url): (String, String)| {
				let mut result = Ok(());
				CTX.with_borrow(|ctx| {
					ctx.as_ref().map(|ctx| {
						let source = Rc::new(VirtualSource::new(
							ctx.location.clone(),
							"Link Display".to_string(),
							display,
						));
						let display_content =
							match util::parse_paragraph(ctx.state, source, ctx.document) {
								Err(err) => {
									result = Err(BadArgument {
										to: Some("push".to_string()),
										pos: 1,
										name: Some("display".to_string()),
										cause: Arc::new(mlua::Error::external(format!(
											"Failed to parse link display: {err}"
										))),
									});
									return;
								}
								Ok(mut paragraph) => {
									std::mem::take(&mut paragraph.content)
								}
							};

						ctx.state.push(
							ctx.document,
							Box::new(Link {
								location: ctx.location.clone(),
								display: display_content,
								url,
							}),
						);
					})
				});

				result
			})
			.unwrap(),
		));

		bindings
	}
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
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

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

	#[test]
	fn lua() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
Some %<nml.link.push("link", "url")>%.
%<
nml.link.push("**BOLD link**", "another url")
>%
			"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

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
