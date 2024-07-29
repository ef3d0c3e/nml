use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::parser::Parser;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
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
	location: Token,
	name: String, // Link name
	url: String,  // Link url
}

impl Link {
	pub fn new(location: Token, name: String, url: String) -> Self {
		Self {
			location: location,
			name,
			url,
		}
	}
}

impl Element for Link {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Inline }
	fn element_name(&self) -> &'static str { "Link" }
	fn to_string(&self) -> String { format!("{self:#?}") }
	fn compile(&self, compiler: &Compiler, _document: &dyn Document) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => Ok(format!(
				"<a href=\"{}\">{}</a>",
				Compiler::sanitize(compiler.target(), self.url.as_str()),
				Compiler::sanitize(compiler.target(), self.name.as_str()),
			)),
			Target::LATEX => Ok(format!(
				"\\href{{{}}}{{{}}}",
				Compiler::sanitize(compiler.target(), self.url.as_str()),
				Compiler::sanitize(compiler.target(), self.name.as_str()),
			)),
		}
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
		document: &'a dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut result = vec![];
		let link_name = match matches.get(1) {
			Some(name) => {
				if name.as_str().is_empty() {
					result.push(
						Report::build(ReportKind::Error, token.source(), name.start())
							.with_message("Empty link name")
							.with_label(
								Label::new((token.source().clone(), name.range()))
									.with_message("Link name is empty")
									.with_color(parser.colors().error),
							)
							.finish(),
					);
					return result;
				}
				// TODO: process into separate document...
				let text_content = util::process_text(document, name.as_str());

				if text_content.as_str().is_empty() {
					result.push(
						Report::build(ReportKind::Error, token.source(), name.start())
							.with_message("Empty link name")
							.with_label(
								Label::new((token.source(), name.range()))
									.with_message(format!(
										"Link name is empty. Once processed, `{}` yields `{}`",
										name.as_str().fg(parser.colors().highlight),
										text_content.as_str().fg(parser.colors().highlight),
									))
									.with_color(parser.colors().error),
							)
							.finish(),
					);
					return result;
				}
				text_content
			}
			_ => panic!("Empty link name"),
		};

		let link_url = match matches.get(2) {
			Some(url) => {
				if url.as_str().is_empty() {
					result.push(
						Report::build(ReportKind::Error, token.source(), url.start())
							.with_message("Empty link url")
							.with_label(
								Label::new((token.source(), url.range()))
									.with_message("Link url is empty")
									.with_color(parser.colors().error),
							)
							.finish(),
					);
					return result;
				}
				let text_content = util::process_text(document, url.as_str());

				if text_content.as_str().is_empty() {
					result.push(
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
					return result;
				}
				text_content
			}
			_ => panic!("Empty link url"),
		};

		parser.push(
			document,
			Box::new(Link::new(token.clone(), link_name, link_url)),
		);

		return result;
	}

	// TODO
	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Vec<(String, Function<'lua>)> { vec![] }
}
