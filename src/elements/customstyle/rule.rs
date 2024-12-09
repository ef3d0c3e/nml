use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use ariadne::Fmt;
use lsp::semantic::Semantics;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use parser::rule::Rule;
use parser::source::Token;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;

use crate::document::document::Document;
use crate::lua::kernel::CTX;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::reports::Report;
use crate::parser::source::Cursor;

use super::custom::CustomStyle;
use super::custom::CustomStyleToken;
use super::custom::LuaCustomStyle;
use super::state::CustomStyleState;
use super::state::STATE_NAME;

#[auto_registry::auto_registry(registry = "rules")]
#[derive(Default)]
pub struct CustomStyleRule;

impl Rule for CustomStyleRule {
	fn name(&self) -> &'static str { "Custom Style" }

	fn previous(&self) -> Option<&'static str> { Some("Style") }

	fn next_match(
		&self,
		_mode: &ParseMode,
		state: &ParserState,
		cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any>)> {
		let content = cursor.source.content();

		let mut closest_match = usize::MAX;
		let mut matched_style = (None, false);
		state
			.shared
			.custom_styles
			.borrow()
			.iter()
			.for_each(|(_name, style)| match style.tokens() {
				CustomStyleToken::Toggle(s) => {
					if let Some(pos) = &content[cursor.pos..].find(s) {
						if *pos < closest_match {
							closest_match = *pos;
							matched_style = (Some(style.clone()), false);
						}
					}
				}
				CustomStyleToken::Pair(begin, end) => {
					if let Some(pos) = &content[cursor.pos..].find(begin) {
						if *pos < closest_match {
							closest_match = *pos;
							matched_style = (Some(style.clone()), false);
						}
					}

					if let Some(pos) = &content[cursor.pos..].find(end) {
						if *pos < closest_match {
							closest_match = *pos;
							matched_style = (Some(style.clone()), true);
						}
					}
				}
			});

		if closest_match == usize::MAX {
			None
		} else {
			Some((
				closest_match + cursor.pos,
				Box::new((matched_style.0.unwrap().clone(), matched_style.1)) as Box<dyn Any>,
			))
		}
	}

	fn on_match<'a>(
		&self,
		state: &ParserState,
		document: &'a dyn Document<'a>,
		cursor: Cursor,
		match_data: Box<dyn Any>,
	) -> (Cursor, Vec<Report>) {
		let (style, end) = match_data
			.downcast_ref::<(Rc<dyn CustomStyle>, bool)>()
			.unwrap();

		let mut rule_state_borrow = state.shared.rule_state.borrow_mut();
		let style_state = match rule_state_borrow.get(STATE_NAME) {
			Some(rule_state) => rule_state,
			// Insert as a new state
			None => {
				match rule_state_borrow.insert(
					STATE_NAME.into(),
					Rc::new(RefCell::new(CustomStyleState {
						toggled: HashMap::new(),
					})),
				) {
					Err(err) => panic!("{err}"),
					Ok(rule_state) => rule_state,
				}
			}
		};

		let (close, token) = match style.tokens() {
			CustomStyleToken::Toggle(s) => {
				let mut borrow = style_state.as_ref().borrow_mut();
				let style_state = borrow.downcast_mut::<CustomStyleState>().unwrap();

				if style_state.toggled.get(style.name()).is_some() {
					// Terminate style
					let token = Token::new(cursor.pos..cursor.pos + s.len(), cursor.source.clone());

					style_state.toggled.remove(style.name());
					(true, token)
				} else {
					// Start style
					let token = Token::new(cursor.pos..cursor.pos + s.len(), cursor.source.clone());

					style_state
						.toggled
						.insert(style.name().into(), token.clone());
					(false, token)
				}
			}
			CustomStyleToken::Pair(s_begin, s_end) => {
				let mut borrow = style_state.borrow_mut();
				let style_state = borrow.downcast_mut::<CustomStyleState>().unwrap();
				if *end {
					// Terminate style
					let token =
						Token::new(cursor.pos..cursor.pos + s_end.len(), cursor.source.clone());
					if style_state.toggled.get(style.name()).is_none() {
						let mut reports = vec![];
						report_err!(
							&mut reports,
							token.source(),
							"Invalid End of Style".into(),
							span(
								token.range.clone(),
								format!(
									"Cannot end style {} here, it does not started anywhere",
									style.name().fg(state.parser.colors().info)
								)
							)
						);
						return (cursor.at(cursor.pos + s_end.len()), reports);
					}

					style_state.toggled.remove(style.name());
					(true, token)
				} else {
					// Start style
					let token = Token::new(
						cursor.pos..cursor.pos + s_begin.len(),
						cursor.source.clone(),
					);
					if let Some(start_token) = style_state.toggled.get(style.name()) {
						let mut reports = vec![];
						report_err!(
							&mut reports,
							token.source(),
							"Invalid Start of Style".into(),
							span(
								token.range.clone(),
								format!(
									"When trying to start custom style {}",
									self.name().fg(state.parser.colors().info)
								)
							),
							span(
								start_token.range.clone(),
								format!(
									"Style {} previously starts here",
									self.name().fg(state.parser.colors().info)
								)
							),
						);
						return (cursor.at(cursor.pos + s_end.len()), reports);
					}

					style_state
						.toggled
						.insert(style.name().into(), token.clone());
					(false, token)
				}
			}
		};

		let reports = if close {
			style.on_end(token.clone(), state, document)
		} else {
			style.on_start(token.clone(), state, document)
		};

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			sems.add(token.range.clone(), tokens.customstyle_marker);
		}

		(cursor.at(token.end()), unsafe {
			std::mem::transmute(reports)
		})
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];

		bindings.push((
			"define_toggled".into(),
			lua.create_function(
				|_,
				 (name, token, on_start, on_end): (
					String,
					String,
					mlua::Function,
					mlua::Function,
				)| {
					let mut result = Ok(());

					let style = LuaCustomStyle {
						tokens: CustomStyleToken::Toggle(token),
						name: name.clone(),
						start: unsafe { std::mem::transmute(on_start.clone()) },
						end: unsafe { std::mem::transmute(on_end.clone()) },
					};

					CTX.with_borrow(|ctx| {
						ctx.as_ref().map(|ctx| {
							if let Some(_) =
								ctx.state.shared.custom_styles.borrow().get(name.as_str())
							{
								result = Err(BadArgument {
									to: Some("define_toggled".to_string()),
									pos: 1,
									name: Some("name".to_string()),
									cause: Arc::new(mlua::Error::external(format!(
										"Custom style with name `{name}` already exists"
									))),
								});
								return;
							}
							ctx.state
								.shared
								.custom_styles
								.borrow_mut()
								.insert(Rc::new(style));

							ctx.state.reset_match("Custom Style").unwrap();
						});
					});

					result
				},
			)
			.unwrap(),
		));

		bindings.push((
			"define_paired".into(),
			lua.create_function(
				|_,
				 (name, token_start, token_end, on_start, on_end): (
					String,
					String,
					String,
					mlua::Function,
					mlua::Function,
				)| {
					let mut result = Ok(());

					if token_start == token_end
					{
						return Err(BadArgument {
							to: Some("define_paired".to_string()),
							pos: 3,
							name: Some("token_end".to_string()),
							cause: Arc::new(mlua::Error::external(format!(
										"Custom style with name `{name}` cannot be defined: The start token must differ from the end token, use `define_toggled` insteda"
							))),
						});
					}

					let style = LuaCustomStyle {
						tokens: CustomStyleToken::Pair(token_start, token_end),
						name: name.clone(),
						start: unsafe { std::mem::transmute(on_start.clone()) },
						end: unsafe { std::mem::transmute(on_end.clone()) },
					};

					CTX.with_borrow(|ctx| {
						ctx.as_ref().map(|ctx| {
							if let Some(_) = ctx.state.shared.custom_styles.borrow().get(name.as_str()) {
								result = Err(BadArgument {
									to: Some("define_paired".to_string()),
									pos: 1,
									name: Some("name".to_string()),
									cause: Arc::new(mlua::Error::external(format!(
										"Custom style with name `{name}` already exists"
									))),
								});
								return;
							}
							ctx.state.shared.custom_styles.borrow_mut().insert(Rc::new(style));

							ctx.state.reset_match("Custom Style").unwrap();
						});
					});

					result
				},
			)
			.unwrap(),
		));

		bindings
	}
}
