use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use document::document::Document;
use elements::customstyle::state::STATE_NAME;
use elements::style::elem::Style;
use elements::style::state::StyleState;
use lsp::conceal::ConcealTarget;
use lsp::conceal::Conceals;
use lsp::semantic::Semantics;
use lsp::styles::Styles;
use lua::kernel::CTX;
use mlua::Error::BadArgument;
use mlua::Function;
use parser::parser::ParseMode;
use parser::parser::ParserState;
use parser::rule::RegexRule;
use parser::source::Token;
use regex::Captures;
use regex::Regex;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
#[auto_registry::auto_registry(registry = "rules")]
pub struct StyleRule {
	re: [Regex; 4],
}

impl Default for StyleRule {
	fn default() -> Self {
		Self {
			re: [
				// Bold
				Regex::new(r"\*\*").unwrap(),
				// Italic
				Regex::new(r"\*").unwrap(),
				// Underline
				Regex::new(r"__").unwrap(),
				// Code
				Regex::new(r"`").unwrap(),
			],
		}
	}
}

impl RegexRule for StyleRule {
	fn name(&self) -> &'static str {
		"Style"
	}

	fn previous(&self) -> Option<&'static str> {
		Some("Table")
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
		_matches: Captures,
	) -> Vec<Report> {
		let query = state.shared.rule_state.borrow().get(STATE_NAME);
		let style_state = match query {
			Some(state) => state,
			None => {
				// Insert as a new state
				match state
					.shared
					.rule_state
					.borrow_mut()
					.insert(STATE_NAME.into(), Rc::new(RefCell::new(StyleState::new())))
				{
					Err(_) => panic!("Unknown error"),
					Ok(state) => state,
				}
			}
		};

		if let Some(style_state) = style_state.borrow_mut().downcast_mut::<StyleState>() {
			let start = style_state.toggled[index].clone();

			style_state.toggled[index] = style_state.toggled[index]
				.clone()
				.map_or(Some(token.clone()), |_| None);
			state.push(
				document,
				Box::new(Style {
					location: token.clone(),
					kind: index,
					close: style_state.toggled[index].is_none(),
				}),
			);

			if let Some(start) = start {
				if let Some(styles) = Styles::from_source(token.source(), &state.shared.lsp) {
					match index {
						0 => styles.add(
							start.start()..token.end(),
							crate::lsp::styles::Style::Group("Bold".into()),
						),
						1 => styles.add(
							start.start()..token.end(),
							crate::lsp::styles::Style::Group("Italic".into()),
						),
						2 => styles.add(
							start.start()..token.end(),
							crate::lsp::styles::Style::Group("Underline".into()),
						),
						3 => styles.add(
							start.start()..token.end(),
							crate::lsp::styles::Style::Group("Code".into()),
						),
						_ => {}
					}
				}
			}

			// Style
			if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp)
			{
				sems.add(token.range.clone(), tokens.style_marker);
			}

			// Conceals
			if let Some(conceals) = Conceals::from_source(token.source(), &state.shared.lsp) {
				conceals.add(token.range.clone(), ConcealTarget::Text("".into()));
			}
		} else {
			panic!("Invalid state at `{STATE_NAME}`");
		}

		vec![]
	}

	fn register_bindings<'lua>(&self, lua: &'lua mlua::Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];

		bindings.push((
			"toggle".to_string(),
			lua.create_function(|_, style: String| {
				let kind = match style.as_str() {
					"bold" | "Bold" => 0,
					"italic" | "Italic" => 1,
					"underline" | "Underline" => 2,
					"emphasis" | "Emphasis" => 3,
					_ => {
						return Err(BadArgument {
							to: Some("toggle".to_string()),
							pos: 1,
							name: Some("style".to_string()),
							cause: Arc::new(mlua::Error::external(
								"Unknown style specified".to_string(),
							)),
						})
					}
				};

				CTX.with_borrow(|ctx| {
					ctx.as_ref().map(|ctx| {
						let query = ctx.state.shared.rule_state.borrow().get(STATE_NAME);
						let style_state = match query {
							Some(state) => state,
							None => {
								// Insert as a new state
								match ctx.state.shared.rule_state.borrow_mut().insert(
									STATE_NAME.into(),
									Rc::new(RefCell::new(StyleState::new())),
								) {
									Err(_) => panic!("Unknown error"),
									Ok(state) => state,
								}
							}
						};

						if let Some(style_state) =
							style_state.borrow_mut().downcast_mut::<StyleState>()
						{
							style_state.toggled[kind] = style_state.toggled[kind]
								.clone()
								.map_or(Some(ctx.location.clone()), |_| None);
							ctx.state.push(
								ctx.document,
								Box::new(Style {
									location: ctx.location.clone(),
									kind,
									close: style_state.toggled[kind].is_none(),
								}),
							);
						} else {
							panic!("Invalid state at `{STATE_NAME}`");
						};
					})
				});

				Ok(())
			})
			.unwrap(),
		));

		bindings
	}
}
