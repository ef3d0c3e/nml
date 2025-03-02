use std::rc::Rc;
use std::sync::Arc;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;
use ariadne::Fmt;
use lua::kernel::Kernel;
use mlua::Error::BadArgument;
use parser::rule::RegexRule;
use parser::source::Token;
use parser::source::VirtualSource;
use parser::state::ParseMode;
use parser::util;
use parser::util::escape_source;
use parser::util::parse_paragraph;
use regex::Regex;
use url::Url;

use super::elem::Link;

#[auto_registry::auto_registry(registry = "rules")]
pub struct LinkRule {
	re: [Regex; 1],
}

impl Default for LinkRule {
	fn default() -> Self {
		Self {
			re: [
				Regex::new(r"\[((?:\\.|[^\\\\])*?)\]\(((?:\\.|[^\\\\])*?)\)").unwrap()
			],
		}
	}
}

impl RegexRule for LinkRule {
	fn name(&self) -> &'static str { "Link" }

	fn previous(&self) -> Option<&'static str> { Some("Link") }

	fn regexes(&self) -> &[Regex] { &self.re }

	fn enabled(&self, _mode: &ParseMode, _id: usize) -> bool { true }

	fn on_regex_match<'u>(
		&self,
		_: usize,
		unit: &mut TranslationUnit<'u>,
		token: Token,
		matches: regex::Captures,
	) {
		// Parse display
		let link_display = match matches.get(1) {
			Some(display) => {
				if display.as_str().is_empty() {
					report_err!(
						unit,
						token.source(),
						"Empty Link Display".into(),
						span(display.range(), "Link display is empty".into())
					);
					return;
				}
				let display_source = escape_source(
					token.source(),
					display.range(),
					"Link Display".into(),
					'\\',
					"]",
				);
				if display_source.content().is_empty() {
					report_err!(
						unit,
						token.source(),
						"Empty Link Display".into(),
						span(
							display.range(),
							format!(
								"Link name is empty. Once processed, `{}` yields `{}`",
								display.as_str().fg(unit.colors().highlight),
								display_source.fg(unit.colors().highlight),
							)
						)
					);
					return;
				}

				unit.with_lsp(|lsp| {
					lsp.with_semantics(token.source(), |sems, tokens| {
						sems.add(
							display.range().start - 1..display.range().start,
							tokens.link_display_sep,
						);
					});
				});

				match parse_paragraph(unit, display_source) {
					Err(err) => {
						report_err!(
							unit,
							token.source(),
							"Invalid Link Display".into(),
							span(
								display.range(),
								format!("Failed to parse link display:\n{err}")
							)
						);
						return;
					}
					Ok(paragraph) => paragraph,
				}
			}
			_ => panic!("Empty link name"),
		};

		// Parse url
		let url_text = matches.get(2).unwrap();
		let url = match Url::parse(util::transform_text(url_text.as_str()).as_str()) {
			Ok(url) => url,
			Err(err) => {
				report_err!(
					unit,
					token.source(),
					"Invalid Link URL".into(),
					span(url_text.range(), err.to_string())
				);
				return;
			},
		};

		// Add element
		unit.add_content(Rc::new(Link {
			location: token.clone(),
			display: vec![link_display],
			url,
		}));

		// Add semantics
		unit.with_lsp(|lsp| lsp.with_semantics(token.source(), |sems, tokens| {
			sems.add(
				matches.get(1).unwrap().end()..matches.get(1).unwrap().end() + 1,
				tokens.link_display_sep,
			);
			let url = matches.get(2).unwrap().range();
			sems.add(url.start - 1..url.start, tokens.link_url_sep);
			sems.add(url.clone(), tokens.link_url);
			sems.add(url.end..url.end + 1, tokens.link_url_sep);
		}));
	}

	fn register_bindings(&self, kernel: &Kernel, table: mlua::Table) {
		kernel.create_function(table, "push", |mut ctx, _, (display, url): (String, String)|  {
			// Parse display
			let source = Arc::new(VirtualSource::new(
					ctx.location.clone(),
					":LUA:Link Display".to_string(),
					display,
			));
			let display_content = match parse_paragraph(ctx.unit, source)
			{
				Err(err) => {
					return Err(BadArgument {
						to: Some("push".to_string()),
						pos: 1,
						name: Some("display".to_string()),
						cause: Arc::new(mlua::Error::external(format!(
									"Failed to parse link display: {err}"
						))),
					});
				}
				Ok(scope) => scope,
			};

			// Parse url
			let url = url::Url::parse(&url).map_err(|err| {
				BadArgument {
					to: Some("push".to_string()),
					pos: 2,
					name: Some("url".to_string()),
					cause: Arc::new(mlua::Error::external(format!(
								"Failed to parse url: {err}"
					))),
				}
			})?;

			// Add element
			let location = ctx.location.clone();
			ctx.unit.add_content(Rc::new(Link {
				location,
				display: vec![display_content],
				url,
			}));

			Ok(())
		});
	}
}
