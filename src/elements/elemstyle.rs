use std::any::Any;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use mlua::Value;
use regex::Regex;

use crate::document::document::Document;
use crate::lua::kernel::CTX;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::source::Source;

pub struct ElemStyleRule {
	start_re: Regex,
}

impl ElemStyleRule {
	pub fn new() -> Self {
		Self {
			start_re: Regex::new(r"(?:^|\n)@@(.*?)=\s*\{").unwrap(),
		}
	}

	/// Finds the json substring inside aother string
	pub fn json_substring(str: &str) -> Option<&str> {
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
}

impl Rule for ElemStyleRule {
	fn name(&self) -> &'static str { "Element Style" }

	fn next_match(&self, _state: &ParserState, cursor: &Cursor) -> Option<(usize, Box<dyn Any>)> {
		self.start_re
			.find_at(cursor.source.content(), cursor.pos)
			.map_or(None, |m| {
				Some((m.start(), Box::new([false; 0]) as Box<dyn Any>))
			})
	}

	fn on_match<'a>(
		&self,
		state: &mut ParserState,
		_document: &'a (dyn Document<'a> + 'a),
		cursor: Cursor,
		_match_data: Option<Box<dyn Any>>,
	) -> (Cursor, Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>) {
		let mut reports = vec![];
		let matches = self
			.start_re
			.captures_at(cursor.source.content(), cursor.pos)
			.unwrap();
		let mut cursor = cursor.at(matches.get(0).unwrap().end() - 1);

		let style = if let Some(key) = matches.get(1) {
			let trimmed = key.as_str().trim_start().trim_end();

			// Check if empty
			if trimmed.is_empty() {
				reports.push(
					Report::build(ReportKind::Error, cursor.source.clone(), key.start())
						.with_message("Empty Style Key")
						.with_label(
							Label::new((cursor.source.clone(), key.range()))
								.with_message(format!("Expected a non-empty style key",))
								.with_color(state.parser.colors().error),
						)
						.finish(),
				);
				return (cursor, reports);
			}

			// Check if key exists
			if !state.shared.style.is_registered(trimmed) {
				reports.push(
					Report::build(ReportKind::Error, cursor.source.clone(), key.start())
						.with_message("Unknown Style Key")
						.with_label(
							Label::new((cursor.source.clone(), key.range()))
								.with_message(format!(
									"Could not find a style with key: {}",
									trimmed.fg(state.parser.colors().info)
								))
								.with_color(state.parser.colors().error),
						)
						.finish(),
				);

				return (cursor, reports);
			}

			state.shared.style.current_style(trimmed)
		} else {
			panic!("Unknown error")
		};

		// Get value
		let new_style = match ElemStyleRule::json_substring(
			&cursor.source.clone().content().as_str()[cursor.pos..],
		) {
			None => {
				reports.push(
					Report::build(ReportKind::Error, cursor.source.clone(), cursor.pos)
						.with_message("Invalid Style Value")
						.with_label(
							Label::new((cursor.source.clone(), matches.get(0).unwrap().range()))
								.with_message(format!(
									"Unable to parse json string after style key",
								))
								.with_color(state.parser.colors().error),
						)
						.finish(),
				);
				return (cursor, reports);
			}
			Some(json) => {
				cursor = cursor.at(cursor.pos + json.len());

				// Attempt to deserialize
				match style.from_json(json) {
					Err(err) => {
						reports.push(
							Report::build(ReportKind::Error, cursor.source.clone(), cursor.pos)
								.with_message("Invalid Style Value")
								.with_label(
									Label::new((
										cursor.source.clone(),
										cursor.pos..cursor.pos + json.len(),
									))
									.with_message(format!(
										"Failed to serialize `{}` into style with key `{}`: {err}",
										json.fg(state.parser.colors().highlight),
										style.key().fg(state.parser.colors().info)
									))
									.with_color(state.parser.colors().error),
								)
								.finish(),
						);
						return (cursor, reports);
					}
					Ok(style) => style,
				}
			}
		};

		state.shared.styles.set_current(new_style);

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
						if !ctx.parser.is_style_registered(style_key.as_str()) {
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

						let style = ctx.parser.current_style(style_key.as_str());
						let new_style = match style.from_lua(lua, new_style) {
							Err(err) => {
								result = Err(err);
								return;
							}
							Ok(new_style) => new_style,
						};

						ctx.parser.set_current_style(new_style);
					})
				});

				result
			})
			.unwrap(),
		));

		bindings
	}
}
