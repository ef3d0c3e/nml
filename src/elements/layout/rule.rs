use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use ariadne::Fmt;
use document::document::Document;
use lsp::hints::Hints;
use lsp::semantic::Semantics;
use lua::kernel::CTX;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use parser::parser::ParseMode;
use parser::parser::ParserState;
use parser::rule::RegexRule;
use parser::source::Source;
use parser::source::Token;
use parser::source::VirtualSource;
use parser::state::RuleState;
use parser::util::escape_source;
use regex::Captures;
use regex::Match;
use regex::Regex;
use regex::RegexBuilder;

use super::custom::LayoutToken;
use super::data::LayoutType;
use super::elem::Layout;
use super::state::LayoutState;
use super::state::STATE_NAME;

pub fn initialize_state(state: &ParserState) -> Rc<RefCell<dyn RuleState>> {
	let mut rule_state_borrow = state.shared.rule_state.borrow_mut();
	match rule_state_borrow.get(STATE_NAME) {
		Some(state) => state,
		None => {
			// Insert as a new state
			match rule_state_borrow.insert(
				STATE_NAME.into(),
				Rc::new(RefCell::new(LayoutState { stack: vec![] })),
			) {
				Err(err) => panic!("{err}"),
				Ok(state) => state,
			}
		}
	}
}

pub fn parse_properties<'a>(
	mut reports: &mut Vec<Report>,
	state: &ParserState,
	token: &Token,
	layout_type: Rc<dyn LayoutType>,
	m: Option<Match>,
) -> Option<Box<dyn Any>> {
	let prop_source = escape_source(
		token.source(),
		m.map_or(0..0, |m| m.range()),
		format!("Layout {} Properties", layout_type.name()),
		'\\',
		"]",
	);
	layout_type.parse_properties(
		&mut reports,
		state,
		Token::new(0..prop_source.content().len(), prop_source),
	)
}

#[auto_registry::auto_registry(registry = "rules")]
pub struct LayoutRule {
	re: [Regex; 3],
}

impl Default for LayoutRule {
	fn default() -> Self {
		Self {
			re: [
				RegexBuilder::new(
					r"(?:^|\n)(?:[^\S\n]*)#\+LAYOUT_BEGIN(?:\[((?:\\.|[^\\\\])*?)\])?(.*)",
				)
				.multi_line(true)
				.build()
				.unwrap(),
				RegexBuilder::new(
					r"(?:^|\n)(?:[^\S\n]*)#\+LAYOUT_NEXT(?:\[((?:\\.|[^\\\\])*?)\])?$",
				)
				.multi_line(true)
				.build()
				.unwrap(),
				RegexBuilder::new(
					r"(?:^|\n)(?:[^\S\n]*)#\+LAYOUT_END(?:\[((?:\\.|[^\\\\])*?)\])?$",
				)
				.multi_line(true)
				.build()
				.unwrap(),
			],
		}
	}
}

impl RegexRule for LayoutRule {
	fn name(&self) -> &'static str { "Layout" }

	fn previous(&self) -> Option<&'static str> { Some("Media") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn enabled(&self, mode: &ParseMode, _id: usize) -> bool { !mode.paragraph_only }

	fn on_regex_match(
		&self,
		index: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		let rule_state = initialize_state(state);

		if index == 0
		// BEGIN_LAYOUT
		{
			match matches.get(2) {
				None => {
					report_err!(
						&mut reports,
						token.source(),
						"Missing Layout Name".into(),
						span(
							token.start() + 1..token.end(),
							format!(
								"Missing layout name after `{}`",
								"#+BEGIN_LAYOUT".fg(state.parser.colors().highlight)
							)
						)
					);
					return reports;
				}
				Some(name) => {
					let trimmed = name.as_str().trim_start().trim_end();
					if name.as_str().is_empty() || trimmed.is_empty()
					// Empty name
					{
						report_err!(
							&mut reports,
							token.source(),
							"Empty Layout Name".into(),
							span(
								name.range(),
								format!(
									"Empty layout name after `{}`",
									"#+BEGIN_LAYOUT".fg(state.parser.colors().highlight)
								)
							)
						);
						return reports;
					} else if !name.as_str().chars().next().unwrap().is_whitespace()
					// Missing space
					{
						report_err!(
							&mut reports,
							token.source(),
							"Invalid Layout Name".into(),
							span(
								name.range(),
								format!(
									"Missing a space before layout name `{}`",
									name.as_str().fg(state.parser.colors().highlight)
								)
							)
						);
						return reports;
					}

					// Get layout
					let layout_type = match state.shared.layouts.borrow().get(trimmed) {
						None => {
							report_err!(
								&mut reports,
								token.source(),
								"Unknown Layout".into(),
								span(
									name.range(),
									format!(
										"Cannot find layout `{}`",
										trimmed.fg(state.parser.colors().highlight)
									)
								)
							);
							return reports;
						}
						Some(layout_type) => layout_type,
					};

					// Parse properties
					let properties = match parse_properties(
						&mut reports,
						state,
						&token,
						layout_type.clone(),
						matches.get(1),
					) {
						Some(props) => props,
						None => return reports,
					};

					state.push(
						document,
						Box::new(Layout {
							location: token.clone(),
							layout: layout_type.clone(),
							id: 0,
							token: LayoutToken::Begin,
							properties,
						}),
					);

					rule_state
						.as_ref()
						.borrow_mut()
						.downcast_mut::<LayoutState>()
						.map_or_else(
							|| panic!("Invalid state at: `{STATE_NAME}`"),
							|s| s.stack.push((vec![token.clone()], layout_type.clone())),
						);

					if let Some((sems, tokens)) =
						Semantics::from_source(token.source(), &state.shared.lsp)
					{
						let start = matches
							.get(0)
							.map(|m| {
								m.start() + token.source().content()[m.start()..].find('#').unwrap()
							})
							.unwrap();
						sems.add(start..start + 2, tokens.layout_sep);
						sems.add(
							start + 2..start + 2 + "LAYOUT_BEGIN".len(),
							tokens.layout_token,
						);
						if let Some(props) = matches.get(1).map(|m| m.range()) {
							sems.add(props.start - 1..props.start, tokens.layout_props_sep);
							sems.add(props.end..props.end + 1, tokens.layout_props_sep);
						}
						sems.add(matches.get(2).unwrap().range(), tokens.layout_type);
					}
				}
			};
			return reports;
		}

		let (id, token_type, layout_type, properties) = if index == 1
		// LAYOUT_NEXT
		{
			let mut rule_state_borrow = rule_state.as_ref().borrow_mut();
			let layout_state = rule_state_borrow.downcast_mut::<LayoutState>().unwrap();

			let (tokens, layout_type) = match layout_state.stack.last_mut() {
				None => {
					report_err!(
						&mut reports,
						token.source(),
						"Invalid #+LAYOUT_NEXT".into(),
						span(
							token.start() + 1..token.end(),
							"No active layout found".into()
						)
					);
					return reports;
				}
				Some(last) => last,
			};

			if let Some(hints) = Hints::from_source(token.source(), &state.shared.lsp) {
				hints.add(token.end(), layout_type.name().to_string());
			}

			if layout_type.expects().end < tokens.len()
			// Too many blocks
			{
				let start = &tokens[0];
				report_err!(
					&mut reports,
					token.source(),
					"Unexpected #+LAYOUT_NEXT".into(),
					span(
						token.start() + 1..token.end(),
						format!(
							"Layout expects a maximum of {} blocks, currently at {}",
							layout_type.expects().end.fg(state.parser.colors().info),
							tokens.len().fg(state.parser.colors().info),
						)
					),
					span(
						start.source(),
						start.start() + 1..start.end(),
						format!("Layout starts here",)
					)
				);
				return reports;
			}

			// Parse properties
			let properties = match parse_properties(
				&mut reports,
				state,
				&token,
				layout_type.clone(),
				matches.get(1),
			) {
				Some(props) => props,
				None => return reports,
			};

			if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp)
			{
				let start = matches
					.get(0)
					.map(|m| m.start() + token.source().content()[m.start()..].find('#').unwrap())
					.unwrap();
				sems.add(start..start + 2, tokens.layout_sep);
				sems.add(
					start + 2..start + 2 + "LAYOUT_NEXT".len(),
					tokens.layout_token,
				);
				if let Some(props) = matches.get(1).map(|m| m.range()) {
					sems.add(props.start - 1..props.start, tokens.layout_props_sep);
					sems.add(props.end..props.end + 1, tokens.layout_props_sep);
				}
			}

			tokens.push(token.clone());
			(
				tokens.len() - 1,
				LayoutToken::Next,
				layout_type.clone(),
				properties,
			)
		} else {
			// LAYOUT_END
			let mut rule_state_borrow = rule_state.as_ref().borrow_mut();
			let layout_state = rule_state_borrow.downcast_mut::<LayoutState>().unwrap();

			let (tokens, layout_type) = match layout_state.stack.last_mut() {
				None => {
					report_err!(
						&mut reports,
						token.source(),
						"Invalid #+LAYOUT_END".into(),
						span(
							token.start() + 1..token.end(),
							"No active layout found".into()
						)
					);
					return reports;
				}
				Some(last) => last,
			};

			if let Some(hints) = Hints::from_source(token.source(), &state.shared.lsp) {
				hints.add(token.end(), layout_type.name().to_string());
			}

			if layout_type.expects().start > tokens.len()
			// Not enough blocks
			{
				let start = &tokens[0];
				report_err!(
					&mut reports,
					token.source(),
					"Unexpected #+LAYOUT_END".into(),
					span(
						token.start() + 1..token.end(),
						format!(
							"Layout expects a minimum of {} blocks, currently at {}",
							layout_type.expects().start.fg(state.parser.colors().info),
							tokens.len().fg(state.parser.colors().info),
						)
					),
					span(
						start.source(),
						start.start() + 1..start.end(),
						format!("Layout starts here",)
					)
				);
				return reports;
			}

			// Parse properties
			let properties = match parse_properties(
				&mut reports,
				state,
				&token,
				layout_type.clone(),
				matches.get(1),
			) {
				Some(props) => props,
				None => return reports,
			};

			let layout_type = layout_type.clone();
			let id = tokens.len();
			layout_state.stack.pop();

			if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp)
			{
				let start = matches
					.get(0)
					.map(|m| m.start() + token.source().content()[m.start()..].find('#').unwrap())
					.unwrap();
				sems.add(start..start + 2, tokens.layout_sep);
				sems.add(
					start + 2..start + 2 + "LAYOUT_END".len(),
					tokens.layout_token,
				);
				if let Some(props) = matches.get(1).map(|m| m.range()) {
					sems.add(props.start - 1..props.start, tokens.layout_props_sep);
					sems.add(props.end..props.end + 1, tokens.layout_props_sep);
				}
			}

			(id, LayoutToken::End, layout_type, properties)
		};

		state.push(
			document,
			Box::new(Layout {
				location: token,
				layout: layout_type,
				id,
				token: token_type,
				properties,
			}),
		);

		reports
	}

	// TODO: Add method to create new layouts
	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];

		bindings.push((
			"push".to_string(),
			lua.create_function(
				|_, (token, layout, properties): (String, String, String)| {
					let mut result = Ok(());

					// Parse token
					let layout_token = match LayoutToken::from_str(token.as_str())
					{
						Err(err) => {
							return Err(BadArgument {
								to: Some("push".to_string()),
								pos: 1,
								name: Some("token".to_string()),
								cause: Arc::new(mlua::Error::external(err))
							});
						},
						Ok(token) => token,
					};

					CTX.with_borrow_mut(|ctx| {
						ctx.as_mut().map(|ctx| {
							// Make sure the rule state has been initialized
							let rule_state = initialize_state(ctx.state);

							// Get layout
							//
							let layout_type = match ctx.state.shared.layouts.borrow().get(layout.as_str())
							{
								None => {
									result = Err(BadArgument {
										to: Some("push".to_string()),
										pos: 2,
										name: Some("layout".to_string()),
										cause: Arc::new(mlua::Error::external(format!(
													"Cannot find layout with name `{layout}`"
										))),
									});
									return;
								},
								Some(layout) => layout,
							};

							// Parse properties
							let prop_source = Rc::new(VirtualSource::new(ctx.location.clone(), ":LUA:Layout Properties".into(), properties)) as Rc<dyn Source>;
							let layout_properties = match layout_type.parse_properties(&mut ctx.reports, ctx.state, prop_source.into()) {
								None => {
									result = Err(BadArgument {
										to: Some("push".to_string()),
										pos: 3,
										name: Some("properties".to_string()),
										cause: Arc::new(mlua::Error::external("Failed to parse properties")),
									});
									return;
								},
								Some(properties) => properties,
							};

							let id = match layout_token {
								LayoutToken::Begin => {
									ctx.state.push(
										ctx.document,
										Box::new(Layout {
											location: ctx.location.clone(),
											layout: layout_type.clone(),
											id: 0,
											token: LayoutToken::Begin,
											properties: layout_properties,
										}),
									);

									rule_state
										.as_ref()
										.borrow_mut()
										.downcast_mut::<LayoutState>()
										.map_or_else(
											|| panic!("Invalid state at: `{STATE_NAME}`"),
											|s| s.stack.push((vec![ctx.location.clone()], layout_type.clone())),
										);
									return;
								},
								LayoutToken::Next => {
									let mut state_borrow = rule_state.as_ref().borrow_mut();
									let layout_state = state_borrow.downcast_mut::<LayoutState>().unwrap();

									let (tokens, current_layout_type) = match layout_state.stack.last_mut() {
										None => {
											result = Err(BadArgument {
												to: Some("push".to_string()),
												pos: 1,
												name: Some("token".to_string()),
												cause: Arc::new(mlua::Error::external("Unable set next layout: No active layout found".to_string())),
											});
											return;
										}
										Some(last) => last,
									};

									if !Rc::ptr_eq(&layout_type, current_layout_type) {
										result = Err(BadArgument {
											to: Some("push".to_string()),
											pos: 2,
											name: Some("layout".to_string()),
											cause: Arc::new(mlua::Error::external(format!("Invalid layout next, current layout is {} vs {}",
												current_layout_type.name(),
												layout_type.name())))
										});
										return;
									}

									if layout_type.expects().end < tokens.len()
										// Too many blocks
									{
										result = Err(BadArgument {
											to: Some("push".to_string()),
											pos: 1,
											name: Some("token".to_string()),
											cause: Arc::new(mlua::Error::external(format!("Unable set layout next: layout {} expect at most {} blocks, currently at {} blocks", 
														layout_type.name(),
														layout_type.expects().end,
														tokens.len()
														))),
										});
										return;
									}

									tokens.push(ctx.location.clone());
									tokens.len() - 1
								},
								LayoutToken::End => {
									let mut state_borrow = rule_state.as_ref().borrow_mut();
									let layout_state = state_borrow.downcast_mut::<LayoutState>().unwrap();

									let (tokens, current_layout_type) = match layout_state.stack.last_mut() {
										None => {
											result = Err(BadArgument {
												to: Some("push".to_string()),
												pos: 1,
												name: Some("token".to_string()),
												cause: Arc::new(mlua::Error::external("Unable set layout end: No active layout found".to_string())),
											});
											return;
										}
										Some(last) => last,
									};

									if !Rc::ptr_eq(&layout_type, current_layout_type) {
										result = Err(BadArgument {
											to: Some("push".to_string()),
											pos: 2,
											name: Some("layout".to_string()),
											cause: Arc::new(mlua::Error::external(format!("Invalid layout end, current layout is {} vs {}",
														current_layout_type.name(),
														layout_type.name())))
										});
										return;
									}

									if layout_type.expects().start > tokens.len()
										// Not enough blocks
									{
										result = Err(BadArgument {
											to: Some("push".to_string()),
											pos: 1,
											name: Some("token".to_string()),
											cause: Arc::new(mlua::Error::external(format!("Unable set next layout: layout {} expect at least {} blocks, currently at {} blocks", 
														layout_type.name(),
														layout_type.expects().start,
														tokens.len()
											))),
										});
										return;
									}

									let id = tokens.len();
									layout_state.stack.pop();
									id
								}
							};

							ctx.state.push(
								ctx.document,
								Box::new(Layout {
									location: ctx.location.clone(),
									layout: layout_type.clone(),
									id,
									token: layout_token,
									properties: layout_properties,
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
