use ariadne::Fmt;
use document::document::Document;
use lsp::code::CodeRange;
use lsp::semantic::Semantics;
use lua::kernel::CTX;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use parser::parser::ParseMode;
use parser::parser::ParserState;
use parser::property::Property;
use parser::property::PropertyParser;
use parser::rule::RegexRule;
use parser::source::Token;
use parser::util::escape_source;
use parser::util::escape_text;
use regex::Captures;
use regex::Regex;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;

use super::elem::Tex;
use super::elem::TexKind;
#[auto_registry::auto_registry(registry = "rules")]
pub struct TexRule {
	re: [Regex; 2],
	properties: PropertyParser,
}

impl Default for TexRule {
	fn default() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"env".to_string(),
			Property::new("Tex environment".to_string(), Some("main".to_string())),
		);
		props.insert(
			"kind".to_string(),
			Property::new("Element display kind".to_string(), None),
		);
		props.insert(
			"caption".to_string(),
			Property::new("Latex caption".to_string(), None),
		);
		Self {
			re: [
				Regex::new(r"\$\|(?:\[((?:\\.|[^\\\\])*?)\])?(?:((?:\\.|[^\\\\])*?)\|\$)?")
					.unwrap(),
				Regex::new(r"\$(?:\[((?:\\.|[^\\\\])*?)\])?(?:((?:\\.|[^\\\\])*?)\$)?").unwrap(),
			],
			properties: PropertyParser { properties: props },
		}
	}
}

impl RegexRule for TexRule {
	fn name(&self) -> &'static str {
		"Tex"
	}

	fn previous(&self) -> Option<&'static str> {
		Some("Code")
	}

	fn regexes(&self) -> &[regex::Regex] {
		&self.re
	}

	fn enabled(&self, _mode: &ParseMode, _id: usize) -> bool {
		true
	}

	fn on_regex_match(
		&self,
		index: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		let tex_content = match matches.get(2) {
			// Unterminated `$`
			None => {
				report_err!(
					&mut reports,
					token.source(),
					"Unterminated Tex Code".into(),
					span(
						token.range.clone(),
						format!(
							"Missing terminating `{}` after first `{}`",
							["|$", "$"][index].fg(state.parser.colors().info),
							["$|", "$"][index].fg(state.parser.colors().info)
						)
					)
				);
				return reports;
			}
			Some(content) => {
				let processed = escape_text(
					'\\',
					["|$", "$"][index],
					content.as_str().trim_start().trim_end(),
					true,
				);

				if processed.is_empty() {
					report_err!(
						&mut reports,
						token.source(),
						"Empty Tex Code".into(),
						span(content.range(), "Tex code is empty".into())
					);
				}
				processed
			}
		};

		// Properties
		let prop_source = escape_source(
			token.source(),
			matches.get(1).map_or(0..0, |m| m.range()),
			"Tex Properties".into(),
			'\\',
			"]",
		);
		let properties = match self.properties.parse(
			"Raw Code",
			&mut reports,
			state,
			Token::new(0..prop_source.content().len(), prop_source),
		) {
			Some(props) => props,
			None => return reports,
		};

		let (tex_kind, caption, tex_env) = match (
			properties.get_or(
				&mut reports,
				"kind",
				if index == 1 {
					TexKind::Inline
				} else {
					TexKind::Block
				},
				|_, value| TexKind::from_str(value.value.as_str()),
			),
			properties.get_opt(&mut reports, "caption", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}),
			properties.get(&mut reports, "env", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}),
		) {
			(Some(tex_kind), Some(caption), Some(tex_env)) => (tex_kind, caption, tex_env),
			_ => return reports,
		};

		// Code ranges
		if let Some(coderanges) = CodeRange::from_source(token.source(), &state.shared.lsp) {
			if index == 0 && tex_content.contains('\n') {
				let range = matches
					.get(2)
					.map(|m| {
						if token.source().content().as_bytes()[m.start()] == b'\n' {
							m.start() + 1..m.end()
						} else {
							m.range()
						}
					})
					.unwrap();

				coderanges.add(range, "Latex".into());
			}
		}

		state.push(
			document,
			Box::new(Tex {
				mathmode: index == 1,
				location: token.clone(),
				kind: tex_kind,
				env: tex_env,
				tex: tex_content,
				caption,
			}),
		);

		// Semantics
		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			let range = token.range;
			sems.add(
				range.start..range.start + if index == 0 { 2 } else { 1 },
				tokens.tex_sep,
			);
			if let Some(props) = matches.get(1).map(|m| m.range()) {
				sems.add(props.start - 1..props.start, tokens.tex_props_sep);
				sems.add(props.end..props.end + 1, tokens.tex_props_sep);
			}
			sems.add(matches.get(2).unwrap().range(), tokens.tex_content);
			sems.add(
				range.end - if index == 0 { 2 } else { 1 }..range.end,
				tokens.tex_sep,
			);
		}

		reports
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];
		bindings.push((
			"push_math".to_string(),
			lua.create_function(
				|_, (kind, tex, env, caption): (String, String, Option<String>, Option<String>)| {
					let mut result = Ok(());
					CTX.with_borrow(|ctx| {
						ctx.as_ref().map(|ctx| {
							let kind = match TexKind::from_str(kind.as_str()) {
								Ok(kind) => kind,
								Err(err) => {
									result = Err(BadArgument {
										to: Some("push".to_string()),
										pos: 2,
										name: Some("kind".to_string()),
										cause: Arc::new(mlua::Error::external(format!(
											"Unable to get tex kind: {err}"
										))),
									});
									return;
								}
							};

							ctx.state.push(
								ctx.document,
								Box::new(Tex {
									location: ctx.location.clone(),
									mathmode: true,
									kind,
									env: env.unwrap_or("main".to_string()),
									tex,
									caption,
								}),
							);
						})
					});

					result
				},
			)
			.unwrap(),
		));

		bindings.push((
			"push".to_string(),
			lua.create_function(
				|_, (kind, tex, env, caption): (String, String, Option<String>, Option<String>)| {
					let mut result = Ok(());
					CTX.with_borrow(|ctx| {
						ctx.as_ref().map(|ctx| {
							let kind = match TexKind::from_str(kind.as_str()) {
								Ok(kind) => kind,
								Err(err) => {
									result = Err(mlua::Error::BadArgument {
										to: Some("push".to_string()),
										pos: 2,
										name: Some("kind".to_string()),
										cause: Arc::new(mlua::Error::external(format!(
											"Unable to get tex kind: {err}"
										))),
									});
									return;
								}
							};

							ctx.state.push(
								ctx.document,
								Box::new(Tex {
									location: ctx.location.clone(),
									mathmode: false,
									kind,
									env: env.unwrap_or("main".to_string()),
									tex,
									caption,
								}),
							);
						})
					});

					result
				},
			)
			.unwrap(),
		));

		bindings
	}
}
