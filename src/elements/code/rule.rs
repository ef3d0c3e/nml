use std::collections::HashMap;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use ariadne::Fmt;
use lsp::code::CodeRange;
use lsp::conceal::Conceals;
use lsp::semantic::Semantics;
use lua::kernel::CTX;
use mlua::Function;
use mlua::Lua;
use parser::property::Property;
use parser::util;
use regex::Captures;
use regex::Regex;
use serde_json::json;

use crate::document::document::Document;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::property::PropertyParser;
use crate::parser::reports::Report;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;
use crate::parser::util::escape_source;

use super::elem::Code;
use super::elem::CodeKind;

#[auto_registry::auto_registry(registry = "rules")]
pub struct CodeRule {
	re: [Regex; 2],
	properties: PropertyParser,
}

impl Default for CodeRule {
	fn default() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"line_offset".to_string(),
			Property::new("Line number offset".to_string(), Some("1".to_string())),
		);
		Self {
			re: [
				Regex::new(
					r"(?:^|\n)```(?:\[((?:\\.|[^\\\\])*?)\])?(.*?)(?:,(.*))?\n((?:\\(?:.|\n)|[^\\\\])*?)```",
				)
				.unwrap(),
				Regex::new(
					r"``(?:\[((?:\\.|[^\\\\])*?)\])?(?:([^\r\n`]*?)(?:,|\n))?((?:\\(?:.|\n)|[^\\\\])*?)``",
				)
				.unwrap(),
			],
			properties: PropertyParser { properties: props },
		}
	}
}

impl RegexRule for CodeRule {
	fn name(&self) -> &'static str {
		"Code"
	}

	fn previous(&self) -> Option<&'static str> {
		Some("Block")
	}

	fn regexes(&self) -> &[regex::Regex] {
		&self.re
	}

	fn enabled(&self, mode: &ParseMode, id: usize) -> bool {
		!mode.paragraph_only || id != 0
	}

	fn on_regex_match(
		&self,
		index: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		captures: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		// Properties
		let prop_source = escape_source(
			token.source(),
			captures.get(1).map_or(0..0, |m| m.range()),
			"Code Properties".into(),
			'\\',
			"]",
		);
		let properties =
			match self
				.properties
				.parse("Code", &mut reports, state, prop_source.into())
			{
				Some(props) => props,
				None => return reports,
			};

		let code_lang = match captures.get(2) {
			None => "Plain Text".to_string(),
			Some(lang) => {
				let mut code_lang = lang.as_str().trim_start().trim_end().to_string();
				if code_lang.is_empty() {
					code_lang = "Plain Text".into();
				}
				if Code::get_syntaxes()
					.find_syntax_by_name(code_lang.as_str())
					.is_none()
				{
					report_err!(
						&mut reports,
						token.source(),
						"Unknown Code Language".into(),
						span(
							lang.range(),
							format!(
								"Language `{}` cannot be found",
								code_lang.fg(state.parser.colors().info)
							)
						)
					);

					return reports;
				}

				code_lang
			}
		};

		let mut code_content = if index == 0 {
			util::escape_text('\\', "```", captures.get(4).unwrap().as_str(), false)
		} else {
			util::escape_text(
				'\\',
				"``",
				captures.get(3).unwrap().as_str(),
				!captures.get(3).unwrap().as_str().contains('\n'),
			)
		};
		if code_content.bytes().last() == Some(b'\n')
		// Remove newline
		{
			code_content.pop();
		}

		if code_content.is_empty() {
			report_err!(
				&mut reports,
				token.source(),
				"Empty Code Content".into(),
				span(token.range.clone(), "Code content cannot be empty".into())
			);
			return reports;
		}

		let theme = document
			.get_variable("code.theme")
			.map(|var| var.to_string());

		if index == 0
		// Block
		{
			let code_name = captures.get(3).and_then(|name| {
				let code_name = name.as_str().trim_end().trim_start().to_string();
				(!code_name.is_empty()).then_some(code_name)
			});
			let line_offset = match properties.get(&mut reports, "line_offset", |_, value| {
				value.value.parse::<usize>()
			}) {
				Some(line_offset) => line_offset,
				_ => return reports,
			};

			state.push(
				document,
				Box::new(Code {
					location: token.clone(),
					block: CodeKind::FullBlock,
					language: code_lang.clone(),
					name: code_name.clone(),
					code: code_content,
					theme,
					line_offset,
				}),
			);

			// Code Ranges
			if let Some(coderanges) = CodeRange::from_source(token.source(), &state.shared.lsp) {
				coderanges.add(captures.get(4).unwrap().range(), code_lang.clone());
			}

			// Conceals
			if let Some(conceals) = Conceals::from_source(token.source(), &state.shared.lsp) {
				let range = captures
					.get(0)
					.map(|m| {
						if token.source().content().as_bytes()[m.start()] == b'\n' {
							m.start() + 1..m.end()
						} else {
							m.range()
						}
					})
					.unwrap();
				let start = range.start;
				let end = token.source().content()[start..]
					.find('\n')
					.map_or(token.source().content().len(), |val| start + val);

				conceals.add(
					start..end,
					lsp::conceal::ConcealTarget::Token {
						token: "code".into(),
						params: json!({
							"name": code_name.unwrap_or("".into()),
							"language": code_lang,
						}),
					},
				);

				let range = captures
					.get(0)
					.map(|m| {
						if token.source().content().as_bytes()[m.start()] == b'\n' {
							m.start() + 1..m.end()
						} else {
							m.range()
						}
					})
					.unwrap();
				conceals.add(
					range.end - 3..range.end,
					lsp::conceal::ConcealTarget::Text("".into()),
				);
			}
		} else
		// Maybe inline
		{
			let block = if code_content.contains('\n') {
				CodeKind::MiniBlock
			} else {
				CodeKind::Inline
			};

			state.push(
				document,
				Box::new(Code {
					location: token.clone(),
					block,
					language: code_lang.clone(),
					name: None,
					code: code_content,
					theme,
					line_offset: 1,
				}),
			);

			// Code Ranges
			if let Some(coderanges) = CodeRange::from_source(token.source(), &state.shared.lsp) {
				if block == CodeKind::MiniBlock {
					let range = captures.get(3).unwrap().range();
					coderanges.add(range.start + 1..range.end, code_lang.clone());
				}
			}

			// Conceals
			if let Some(conceals) = Conceals::from_source(token.source(), &state.shared.lsp) {
				if block == CodeKind::MiniBlock {
					let range = captures
						.get(0)
						.map(|m| {
							if token.source().content().as_bytes()[m.start()] == b'\n' {
								m.start() + 1..m.end()
							} else {
								m.range()
							}
						})
						.unwrap();
					let start = range.start;
					let end = token.source().content()[start..]
						.find('\n')
						.map_or(token.source().content().len(), |val| start + val);

					conceals.add(
						start..end,
						lsp::conceal::ConcealTarget::Token {
							token: "code".into(),
							params: json!({
								"name": "".to_string(),
								"language": code_lang,
							}),
						},
					);

					let range = captures
						.get(0)
						.map(|m| {
							if token.source().content().as_bytes()[m.start()] == b'\n' {
								m.start() + 1..m.end()
							} else {
								m.range()
							}
						})
						.unwrap();
					conceals.add(
						range.end - 2..range.end,
						lsp::conceal::ConcealTarget::Text("".into()),
					);
				}
			}
		}

		// Semantic
		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			let range = captures
				.get(0)
				.map(|m| {
					if token.source().content().as_bytes()[m.start()] == b'\n' {
						m.start() + 1..m.end()
					} else {
						m.range()
					}
				})
				.unwrap();
			sems.add(
				range.start..range.start + if index == 0 { 3 } else { 2 },
				tokens.code_sep,
			);
			if let Some(props) = captures.get(1).map(|m| m.range()) {
				sems.add(props.start - 1..props.start, tokens.code_props_sep);
				sems.add(props.end..props.end + 1, tokens.code_props_sep);
			}
			if let Some(lang) = captures.get(2).map(|m| m.range()) {
				sems.add(lang.clone(), tokens.code_lang);
			}
			if index == 0 {
				if let Some(title) = captures.get(3).map(|m| m.range()) {
					sems.add(title.clone(), tokens.code_title);
				}
			}
			sems.add(
				range.end - if index == 0 { 3 } else { 2 }..range.end,
				tokens.code_sep,
			);
		}

		reports
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];
		bindings.push((
			"push_inline".to_string(),
			lua.create_function(|_, (language, content): (String, String)| {
				CTX.with_borrow(|ctx| {
					ctx.as_ref().map(|ctx| {
						let theme = ctx
							.document
							.get_variable("code.theme")
							.map(|var| var.to_string());

						ctx.state.push(
							ctx.document,
							Box::new(Code {
								location: ctx.location.clone(),
								block: CodeKind::Inline,
								language,
								name: None,
								code: content,
								theme,
								line_offset: 1,
							}),
						);
					})
				});

				Ok(())
			})
			.unwrap(),
		));

		bindings.push((
			"push_miniblock".to_string(),
			lua.create_function(
				|_, (language, content, line_offset): (String, String, Option<usize>)| {
					CTX.with_borrow(|ctx| {
						ctx.as_ref().map(|ctx| {
							let theme = ctx
								.document
								.get_variable("code.theme")
								.map(|var| var.to_string());

							ctx.state.push(
								ctx.document,
								Box::new(Code {
									location: ctx.location.clone(),
									block: CodeKind::MiniBlock,
									language,
									name: None,
									code: content,
									theme,
									line_offset: line_offset.unwrap_or(1),
								}),
							);
						})
					});

					Ok(())
				},
			)
			.unwrap(),
		));

		bindings.push((
			"push_block".to_string(),
			lua.create_function(
				|_,
				 (language, name, content, line_offset): (
					String,
					Option<String>,
					String,
					Option<usize>,
				)| {
					CTX.with_borrow(|ctx| {
						ctx.as_ref().map(|ctx| {
							let theme = ctx
								.document
								.get_variable("code.theme")
								.map(|var| var.to_string());

							ctx.state.push(
								ctx.document,
								Box::new(Code {
									location: ctx.location.clone(),
									block: CodeKind::FullBlock,
									language,
									name,
									code: content,
									theme,
									line_offset: line_offset.unwrap_or(1),
								}),
							);
						})
					});

					Ok(())
				},
			)
			.unwrap(),
		));

		bindings
	}
}
