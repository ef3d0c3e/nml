use std::rc::Rc;
use std::sync::Arc;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use ariadne::Fmt;
use document::document::Document;
use lsp::semantic::Semantics;
use lua::kernel::CTX;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use parser::parser::ParseMode;
use parser::parser::ParserState;
use parser::rule::RegexRule;
use parser::source::Token;
use parser::source::VirtualSource;
use parser::util::escape_source;
use parser::util::parse_paragraph;
use parser::util::process_text;
use regex::Captures;
use regex::Regex;

use super::elem::Link;

#[auto_registry::auto_registry(registry = "rules")]
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
	fn name(&self) -> &'static str {
		"Link"
	}

	fn previous(&self) -> Option<&'static str> {
		Some("Link")
	}

	fn regexes(&self) -> &[Regex] {
		&self.re
	}

	fn enabled(&self, _mode: &ParseMode, _id: usize) -> bool {
		true
	}

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
				let display_source = escape_source(
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
				match parse_paragraph(state, display_source, document) {
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
				let text_content = process_text(document, url.as_str());

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
						let display_content = match parse_paragraph(ctx.state, source, ctx.document)
						{
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
