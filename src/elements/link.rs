use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::lsp::semantic::Semantics;
use crate::lua::kernel::CTX;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use crate::parser::util;
use ariadne::Fmt;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use regex::Captures;
use regex::Regex;
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
	fn compile(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		cursor: usize,
	) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				let mut result = format!(
					"<a href=\"{}\">",
					Compiler::sanitize(compiler.target(), self.url.as_str())
				);

				for elem in &self.display {
					result += elem
						.compile(compiler, document, cursor + result.len())?
						.as_str();
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

impl Default for LinkRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r"\[((?:\\.|[^\\\\])*?)\]\(((?:\\.|[^\\\\])*?)\)").unwrap()],
		}
	}
}

impl RegexRule for LinkRule {
	fn name(&self) -> &'static str { "Link" }

	fn previous(&self) -> Option<&'static str> { Some("Link") }

	fn regexes(&self) -> &[Regex] { &self.re }

	fn enabled(&self, _mode: &ParseMode, _id: usize) -> bool { true }

	fn on_regex_match<'a>(
		&self,
		_: usize,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		let link_display = match matches.get(1) {
			Some(display) => {
				if display.as_str().is_empty() {
					report_err!(
						&mut reports,
						token.source(),
						"Empty Link Display".into(),
						span(display.range(), "Link display is empty".into())
					);
					return reports;
				}
				let display_source = util::escape_source(
					token.source(),
					display.range(),
					"Link Display".into(),
					'\\',
					"](",
				);
				if display_source.content().is_empty() {
					report_err!(
						&mut reports,
						token.source(),
						"Empty Link Display".into(),
						span(
							display.range(),
							format!(
								"Link name is empty. Once processed, `{}` yields `{}`",
								display.as_str().fg(state.parser.colors().highlight),
								display_source.fg(state.parser.colors().highlight),
							)
						)
					);
					return reports;
				}

				if let Some((sems, tokens)) =
					Semantics::from_source(token.source(), &state.shared.lsp)
				{
					sems.add(
						display.range().start - 1..display.range().start,
						tokens.link_display_sep,
					);
				}
				match util::parse_paragraph(state, display_source, document) {
					Err(err) => {
						report_err!(
							&mut reports,
							token.source(),
							"Invalid Link Display".into(),
							span(
								display.range(),
								format!("Failed to parse link display:\n{err}")
							)
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
					report_err!(
						&mut reports,
						token.source(),
						"Empty Link URL".into(),
						span(url.range(), "Link url is empty".into())
					);
					return reports;
				}
				let text_content = util::process_text(document, url.as_str());

				if text_content.is_empty() {
					report_err!(
						&mut reports,
						token.source(),
						"Empty Link URL".into(),
						span(
							url.range(),
							format!(
								"Link url is empty. Once processed, `{}` yields `{}`",
								url.as_str().fg(state.parser.colors().highlight),
								text_content.as_str().fg(state.parser.colors().highlight),
							)
						)
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
				location: token.clone(),
				display: link_display,
				url: link_url,
			}),
		);

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			sems.add(
				matches.get(1).unwrap().end()..matches.get(1).unwrap().end() + 1,
				tokens.link_display_sep,
			);
			let url = matches.get(2).unwrap().range();
			sems.add(url.start - 1..url.start, tokens.link_url_sep);
			sems.add(url.clone(), tokens.link_url);
			sems.add(url.end..url.end + 1, tokens.link_url_sep);
		}

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
								Ok(mut paragraph) => std::mem::take(&mut paragraph.content),
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
	use crate::validate_semantics;

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
		let (doc, _) = parser.parse(
			ParserState::new(&parser, None),
			source,
			None,
			ParseMode::default(),
		);

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
		let (doc, _) = parser.parse(
			ParserState::new(&parser, None),
			source,
			None,
			ParseMode::default(),
		);

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
	fn semantics() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
 -  [la\](*testi*nk](url)
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
		link_display_sep { delta_line == 0, delta_start == 3, length == 1 };
		style_marker { delta_line == 0, delta_start == 6, length == 1 };
		style_marker { delta_line == 0, delta_start == 6, length == 1 };
		link_display_sep { delta_line == 0, delta_start == 3, length == 1 };
		link_url_sep { delta_line == 0, delta_start == 1, length == 1 };
		link_url { delta_line == 0, delta_start == 1, length == 3 };
		link_url_sep { delta_line == 0, delta_start == 3, length == 1 };
		);
	}
}
