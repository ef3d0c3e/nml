use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::lsp::semantic::Semantics;
use crate::lua::kernel::CTX;
use crate::parser::layout::LayoutType;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;
use crate::parser::state::RuleState;
use crate::parser::state::Scope;
use ariadne::Fmt;
use lsp::hints::Hints;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use parser::parser::SharedState;
use parser::source::Source;
use parser::source::VirtualSource;
use parser::util::escape_source;
use regex::Captures;
use regex::Match;
use regex::Regex;
use regex::RegexBuilder;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutToken {
	Begin,
	Next,
	End,
}

impl FromStr for LayoutToken {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"Begin" | "begin" => Ok(LayoutToken::Begin),
			"Next" | "next" => Ok(LayoutToken::Next),
			"End" | "end" => Ok(LayoutToken::End),
			_ => Err(format!("Unable to find LayoutToken with name: {s}")),
		}
	}
}

mod default_layouts {
	use crate::parser::layout::LayoutType;
	use crate::parser::property::Property;
	use crate::parser::property::PropertyParser;

	use super::*;

	#[derive(Debug)]
	pub struct Centered(PropertyParser);

	impl Default for Centered {
		fn default() -> Self {
			let mut properties = HashMap::new();
			properties.insert(
				"style".to_string(),
				Property::new(
					"Additional style for the split".to_string(),
					Some("".to_string()),
				),
			);

			Self(PropertyParser { properties })
		}
	}

	impl LayoutType for Centered {
		fn name(&self) -> &'static str { "Centered" }

		fn expects(&self) -> Range<usize> { 1..1 }

		fn parse_properties(
			&self,
			reports: &mut Vec<Report>,
			state: &ParserState,
			token: Token,
		) -> Option<Box<dyn Any>> {
			let properties = match self.0.parse("Centered Layout", reports, state, token) {
				Some(props) => props,
				None => return None,
			};

			let style = match properties.get(reports, "style", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}) {
				Some(style) => style,
				_ => return None,
			};

			Some(Box::new(style))
		}

		fn compile(
			&self,
			token: LayoutToken,
			_id: usize,
			properties: &Box<dyn Any>,
			compiler: &Compiler,
			_document: &dyn Document,
		) -> Result<String, String> {
			match compiler.target() {
				Target::HTML => {
					let style = match properties.downcast_ref::<String>().unwrap().as_str() {
						"" => "".to_string(),
						str => format!(r#" style={}"#, Compiler::sanitize(compiler.target(), str)),
					};
					match token {
						LayoutToken::Begin => Ok(format!(r#"<div class="centered"{style}>"#)),
						LayoutToken::Next => panic!(),
						LayoutToken::End => Ok(r#"</div>"#.to_string()),
					}
				}
				_ => todo!(""),
			}
		}
	}

	#[derive(Debug)]
	pub struct Split(PropertyParser);

	impl Default for Split {
		fn default() -> Self {
			let mut properties = HashMap::new();
			properties.insert(
				"style".to_string(),
				Property::new(
					"Additional style for the split".to_string(),
					Some("".to_string()),
				),
			);

			Self(PropertyParser { properties })
		}
	}

	impl LayoutType for Split {
		fn name(&self) -> &'static str { "Split" }

		fn expects(&self) -> Range<usize> { 2..usize::MAX }

		fn parse_properties(
			&self,
			reports: &mut Vec<Report>,
			state: &ParserState,
			token: Token,
		) -> Option<Box<dyn Any>> {
			let properties = match self.0.parse("Split Layout", reports, state, token) {
				Some(props) => props,
				None => return None,
			};

			let style = match properties.get(reports, "style", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}) {
				Some(style) => style,
				_ => return None,
			};

			Some(Box::new(style))
		}

		fn compile(
			&self,
			token: LayoutToken,
			_id: usize,
			properties: &Box<dyn Any>,
			compiler: &Compiler,
			_document: &dyn Document,
		) -> Result<String, String> {
			match compiler.target() {
				Target::HTML => {
					let style = match properties.downcast_ref::<String>().unwrap().as_str() {
						"" => "".to_string(),
						str => format!(r#" style={}"#, Compiler::sanitize(compiler.target(), str)),
					};
					match token {
						LayoutToken::Begin => Ok(format!(
							r#"<div class="split-container"><div class="split"{style}>"#
						)),
						LayoutToken::Next => Ok(format!(r#"</div><div class="split"{style}>"#)),
						LayoutToken::End => Ok(r#"</div></div>"#.to_string()),
					}
				}
				_ => todo!(""),
			}
		}
	}

	#[derive(Debug)]
	pub struct Spoiler(PropertyParser);

	impl Default for Spoiler {
		fn default() -> Self {
			let mut properties = HashMap::new();
			properties.insert(
				"title".to_string(),
				Property::new("Spoiler title".to_string(), Some("".to_string())),
			);

			Self(PropertyParser { properties })
		}
	}

	impl LayoutType for Spoiler {
		fn name(&self) -> &'static str { "Spoiler" }

		fn expects(&self) -> Range<usize> { 1..1 }

		fn parse_properties(
			&self,
			reports: &mut Vec<Report>,
			state: &ParserState,
			token: Token,
		) -> Option<Box<dyn Any>> {
			let properties = match self.0.parse("Spoiler Layout", reports, state, token) {
				Some(props) => props,
				None => return None,
			};

			let title = match properties.get(reports, "title", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}) {
				Some(title) => title,
				_ => return None,
			};

			Some(Box::new(title))
		}

		fn compile(
			&self,
			token: LayoutToken,
			_id: usize,
			properties: &Box<dyn Any>,
			compiler: &Compiler,
			_document: &dyn Document,
		) -> Result<String, String> {
			match compiler.target() {
				Target::HTML => {
					let title = properties.downcast_ref::<String>().unwrap();
					match token {
						LayoutToken::Begin => Ok(format!(
							r#"<details class="spoiler"><summary>{}</summary>"#,
							Compiler::sanitize(compiler.target(), title)
						)),
						LayoutToken::End => Ok(r#"</details>"#.to_string()),
						_ => panic!(),
					}
				}
				_ => todo!(""),
			}
		}
	}
}

#[derive(Debug)]
struct Layout {
	pub(self) location: Token,
	pub(self) layout: Rc<dyn LayoutType>,
	pub(self) id: usize,
	pub(self) token: LayoutToken,
	pub(self) properties: Box<dyn Any>,
}

impl Element for Layout {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Block }
	fn element_name(&self) -> &'static str { "Layout" }
	fn compile(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		_cursor: usize,
	) -> Result<String, String> {
		self.layout
			.compile(self.token, self.id, &self.properties, compiler, document)
	}
}

struct LayoutState {
	/// The layout stack
	pub(self) stack: Vec<(Vec<Token>, Rc<dyn LayoutType>)>,
}

impl RuleState for LayoutState {
	fn scope(&self) -> Scope { Scope::DOCUMENT }

	fn on_remove(&self, state: &ParserState, document: &dyn Document) -> Vec<Report> {
		let mut reports = vec![];

		let doc_borrow = document.content().borrow();
		let at = doc_borrow.last().map_or(
			Token::new(
				document.source().content().len()..document.source().content().len(),
				document.source(),
			),
			|last| last.location().to_owned(),
		);

		for (tokens, layout_type) in &self.stack {
			let start = tokens.first().unwrap();
			report_err!(
				&mut reports,
				start.source(),
				"Unterminated Layout".into(),
				span(
					start.source(),
					start.range.start + 1..start.range.end,
					format!(
						"Layout {} stars here",
						layout_type.name().fg(state.parser.colors().info)
					)
				),
				span(at.source(), at.range.clone(), "Document ends here".into())
			);
		}

		reports
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::layout")]
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
		reports,
		state,
		Token::new(0..prop_source.content().len(), prop_source),
	)
}

static STATE_NAME: &str = "elements.layout";

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

	fn register_shared_state(&self, state: &SharedState) {
		let mut holder = state.layouts.borrow_mut();
		holder.insert(Rc::new(default_layouts::Centered::default()));
		holder.insert(Rc::new(default_layouts::Split::default()));
		holder.insert(Rc::new(default_layouts::Spoiler::default()));
	}
}

#[cfg(test)]
mod tests {
	use crate::elements::paragraph::Paragraph;
	use crate::elements::text::Text;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;
	use crate::validate_semantics;

	use super::*;

	#[test]
	fn parser() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
#+LAYOUT_BEGIN[style=A] Split
	A
	#+LAYOUT_BEGIN[style=B] Centered
		B
	#+LAYOUT_END
#+LAYOUT_NEXT[style=C]
	C
	#+LAYOUT_BEGIN[style=D] Split
		D
	#+LAYOUT_NEXT[style=E]
		E
	#+LAYOUT_END
#+LAYOUT_END
#+LAYOUT_BEGIN[title=F] Spoiler
	F
#+LAYOUT_END
"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(
			ParserState::new(&parser, None),
			source,
			None,
			ParseMode::default(),
		);

		validate_document!(doc.content().borrow(), 0,
			Layout { token == LayoutToken::Begin, id == 0 };
			Paragraph {
				Text { content == "A" };
			};
			Layout { token == LayoutToken::Begin, id == 0 };
			Paragraph {
				Text { content == "B" };
			};
			Layout { token == LayoutToken::End, id == 1 };
			Layout { token == LayoutToken::Next, id == 1 };
			Paragraph {
				Text { content == "C" };
			};
			Layout { token == LayoutToken::Begin, id == 0 };
			Paragraph {
				Text { content == "D" };
			};
			Layout { token == LayoutToken::Next, id == 1 };
			Paragraph {
				Text { content == "E" };
			};
			Layout { token == LayoutToken::End, id == 2 };
			Layout { token == LayoutToken::End, id == 2 };

			Layout { token == LayoutToken::Begin, id == 0 };
			Paragraph {
				Text { content == "F" };
			};
			Layout { token == LayoutToken::End, id == 1 };
		);
	}

	#[test]
	fn lua() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
%<nml.layout.push("begin", "Split", "style=A")>%
	A
%<nml.layout.push("Begin", "Centered", "style=B")>%
		B
%<nml.layout.push("end", "Centered", "")>%
%<nml.layout.push("next", "Split", "style=C")>%
	C
%<nml.layout.push("Begin", "Split", "style=D")>%
		D
%<nml.layout.push("Next", "Split", "style=E")>%
		E
%<nml.layout.push("End", "Split", "")>%
%<nml.layout.push("End", "Split", "")>%

%<nml.layout.push("Begin", "Spoiler", "title=Test Spoiler")>%
	F
%<nml.layout.push("End", "Spoiler", "")>%
"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(
			ParserState::new(&parser, None),
			source,
			None,
			ParseMode::default(),
		);

		validate_document!(doc.content().borrow(), 0,
			Layout { token == LayoutToken::Begin, id == 0 };
			Paragraph {
				Text { content == "A" };
			};
			Layout { token == LayoutToken::Begin, id == 0 };
			Paragraph {
				Text { content == "B" };
			};
			Layout { token == LayoutToken::End, id == 1 };
			Layout { token == LayoutToken::Next, id == 1 };
			Paragraph {
				Text { content == "C" };
			};
			Layout { token == LayoutToken::Begin, id == 0 };
			Paragraph {
				Text { content == "D" };
			};
			Layout { token == LayoutToken::Next, id == 1 };
			Paragraph {
				Text { content == "E" };
			};
			Layout { token == LayoutToken::End, id == 2 };
			Layout { token == LayoutToken::End, id == 2 };
			Paragraph;
			Layout { token == LayoutToken::Begin, id == 0 };
			Paragraph {
				Text { content == "F" };
			};
			Layout { token == LayoutToken::End, id == 1 };
		);
	}

	#[test]
	fn semantic() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
#+LAYOUT_BEGIN Split
	#+LAYOUT_NEXT[style=aa]
#+LAYOUT_END
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
			layout_sep { delta_line == 1, delta_start == 0, length == 2 };
			layout_token { delta_line == 0, delta_start == 2, length == 12 };
			layout_type { delta_line == 0, delta_start == 12, length == 6 };
			layout_sep { delta_line == 1, delta_start == 1, length == 2 };
			layout_token { delta_line == 0, delta_start == 2, length == 11 };
			layout_props_sep { delta_line == 0, delta_start == 11, length == 1 };
			prop_name { delta_line == 0, delta_start == 1, length == 5 };
			prop_equal { delta_line == 0, delta_start == 5, length == 1 };
			prop_value { delta_line == 0, delta_start == 1, length == 2 };
			layout_props_sep { delta_line == 0, delta_start == 2, length == 1 };
			layout_sep { delta_line == 1, delta_start == 0, length == 2 };
			layout_token { delta_line == 0, delta_start == 2, length == 10 };
		);
	}

	#[test]
	fn hints() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
#+LAYOUT_BEGIN Split
	A
#+LAYOUT_NEXT
	B
#+LAYOUT_END
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
		if let Some(lsp) = &state.shared.lsp {
			let borrow = lsp.borrow();

			if let Some(hints) = borrow.inlay_hints.get(&(source as Rc<dyn Source>)) {
				let borrow = hints.hints.borrow();
				assert_eq!(
					borrow[0].position,
					tower_lsp::lsp_types::Position {
						line: 3,
						character: 13
					}
				);
				assert_eq!(
					borrow[1].position,
					tower_lsp::lsp_types::Position {
						line: 5,
						character: 12
					}
				);
			}
		}
	}
}
