use crate::parser::parser::ParseMode;
use crate::parser::style::ElementStyle;
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;

use ariadne::Fmt;
use lsp::semantic::Semantics;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use mlua::Value;
use regex::Regex;

use crate::document::document::Document;
use crate::lua::kernel::CTX;
use crate::parser::parser::ParserState;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::elemstyle")]
pub struct ElemStyleRule {
	start_re: Regex,
}

impl Default for ElemStyleRule {
	fn default() -> Self {
		Self {
			start_re: Regex::new(r"(?:^|\n)@@(.*?)=\s*\{").unwrap(),
		}
	}
}

/// Finds the json substring inside aother string
fn json_substring(str: &str) -> Option<&str> {
	let mut in_string = false;
	let mut brace_depth = 0;
	let mut escaped = false;

	for (pos, c) in str.char_indices() {
		match c {
			'{' if !in_string => brace_depth += 1,
			'}' if !in_string => brace_depth -= 1,
			'\\' if in_string => escaped = !escaped,
			'"' if !escaped => in_string = !in_string,
			_ => escaped = false,
		}

		if brace_depth == 0 {
			return Some(&str[..=pos]);
		}
	}

	None
}

impl Rule for ElemStyleRule {
	fn name(&self) -> &'static str { "Element Style" }

	fn previous(&self) -> Option<&'static str> { Some("Script") }

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
		_document: &'a (dyn Document<'a> + 'a),
		cursor: Cursor,
		_match_data: Box<dyn Any>,
	) -> (Cursor, Vec<Report>) {
		let mut reports = vec![];
		let matches = self
			.start_re
			.captures_at(cursor.source.content(), cursor.pos)
			.unwrap();
		let mut cursor = cursor.at(matches.get(0).unwrap().end() - 1);

		let style: Rc<dyn ElementStyle> = if let Some(key) = matches.get(1) {
			let trimmed = key.as_str().trim_start().trim_end();

			// Check if empty
			if trimmed.is_empty() {
				report_err!(
					&mut reports,
					cursor.source.clone(),
					"Empty Style Key".into(),
					span(key.range(), "Expected a non-empty style key".into()),
				);
				return (cursor, reports);
			}

			// Check if key exists
			if !state.shared.styles.borrow().is_registered(trimmed) {
				report_err!(
					&mut reports,
					cursor.source.clone(),
					"Unknown Style Key".into(),
					span(
						key.range(),
						format!(
							"Could not find a style with key: {}",
							trimmed.fg(state.parser.colors().info)
						)
					),
				);

				return (cursor, reports);
			}

			state.shared.styles.borrow().current(trimmed)
		} else {
			panic!("Unknown error")
		};

		// Get value
		let new_style =
			match json_substring(&cursor.source.clone().content().as_str()[cursor.pos..]) {
				None => {
					report_err!(
						&mut reports,
						cursor.source.clone(),
						"Invalid Style Value".into(),
						span(
							matches.get(0).unwrap().range(),
							"Unable to parse json string after style key".into()
						)
					);
					return (cursor, reports);
				}
				Some(json) => {
					// Attempt to deserialize
					match style.from_json(json) {
						Err(err) => {
							report_err!(
								&mut reports,
								cursor.source.clone(),
								"Invalid Style Value".into(),
								span(
									cursor.pos..cursor.pos + json.len(),
									format!(
										"Failed to serialize `{}` into style with key `{}`: {err}",
										json.fg(state.parser.colors().highlight),
										style.key().fg(state.parser.colors().info)
									)
								)
							);

							cursor = cursor.at(cursor.pos + json.len());
							return (cursor, reports);
						}
						Ok(style) => {
							if let Some((sems, tokens)) =
								Semantics::from_source(cursor.source.clone(), &state.shared.lsp)
							{
								let style = matches.get(1).unwrap();
								sems.add(
									style.start() - 2..style.start(),
									tokens.elemstyle_operator,
								);
								sems.add(style.range(), tokens.elemstyle_name);
								sems.add(style.end()..style.end() + 1, tokens.elemstyle_equal);
								sems.add(
									matches.get(0).unwrap().end() - 1
										..matches.get(0).unwrap().end() + json.len(),
									tokens.elemstyle_value,
								);
							}

							cursor = cursor.at(cursor.pos + json.len());
							style
						}
					}
				}
			};

		state.shared.styles.borrow_mut().set_current(new_style);

		(cursor, reports)
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];

		bindings.push((
			"set".to_string(),
			lua.create_function(|lua, (style_key, new_style): (String, Value)| {
				let mut result = Ok(());
				CTX.with_borrow(|ctx| {
					ctx.as_ref().map(|ctx| {
						if !ctx
							.state
							.shared
							.styles
							.borrow()
							.is_registered(style_key.as_str())
						{
							result = Err(BadArgument {
								to: Some("set".to_string()),
								pos: 1,
								name: Some("style_key".to_string()),
								cause: Arc::new(mlua::Error::external(format!(
									"Unable to find style with key: {style_key}"
								))),
							});
							return;
						}

						let style = ctx.state.shared.styles.borrow().current(style_key.as_str());
						let new_style = match style.from_lua(lua, new_style) {
							Err(err) => {
								result = Err(err);
								return;
							}
							Ok(new_style) => new_style,
						};

						ctx.state.shared.styles.borrow_mut().set_current(new_style);
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
pub mod tests {
	use parser::langparser::LangParser;
	use parser::parser::Parser;
	use parser::source::SourceFile;

	use super::*;

	#[test]
	fn semantics() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
@@style.section = {
	"link_pos": "Before",
	"link": ["", "⛓️", "       "]
}
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
			elemstyle_operator { delta_line == 1, delta_start == 0, length == 2 };
			elemstyle_name { delta_line == 0, delta_start == 2, length == 14 };
			elemstyle_equal { delta_line == 0, delta_start == 14, length == 1 };
			elemstyle_value { delta_line == 0, delta_start == 2, length == 2 };
			elemstyle_value { delta_line == 1, delta_start == 0, length == 23 };
			elemstyle_value { delta_line == 1, delta_start == 0, length == 31 };
			elemstyle_value { delta_line == 1, delta_start == 0, length == 2 };
		);
	}
}
