use crate::lua::kernel::Kernel;
use crate::parser::parser::ParseMode;
use std::any::Any;
use std::cell::Ref;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use ariadne::Fmt;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;

use crate::document::document::Document;
use crate::document::document::DocumentAccessors;
use crate::lua::kernel::KernelContext;
use crate::lua::kernel::CTX;
use crate::parser::customstyle::CustomStyle;
use crate::parser::customstyle::CustomStyleToken;
use crate::parser::parser::ParserState;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::source::Token;
use crate::parser::state::RuleState;
use crate::parser::state::Scope;

use super::paragraph::Paragraph;

#[derive(Debug)]
struct LuaCustomStyle {
	pub(self) name: String,
	pub(self) tokens: CustomStyleToken,
	pub(self) start: String,
	pub(self) end: String,
}

impl CustomStyle for LuaCustomStyle {
	fn name(&self) -> &str { self.name.as_str() }

	fn tokens(&self) -> &CustomStyleToken { &self.tokens }

	fn on_start<'a>(
		&self,
		location: Token,
		state: &ParserState,
		document: &'a dyn Document<'a>,
	) -> Vec<Report> {
		let kernel: Ref<'_, Kernel> =
			Ref::map(state.shared.kernels.borrow(), |b| b.get("main").unwrap());
		//let kernel = RefMut::map(parser_state.shared.kernels.borrow(), |ker| ker.get("main").unwrap());
		let ctx = KernelContext {
			location: location.clone(),
			state,
			document,
		};

		let mut reports = vec![];
		kernel.run_with_context(ctx, |lua| {
			let chunk = lua.load(self.start.as_str());
			if let Err(err) = chunk.eval::<()>() {
				report_err!(
					&mut reports,
					location.source(),
					"Lua execution failed".into(),
					span(location.range.clone(), err.to_string()),
					note(format!(
						"When trying to start custom style {}",
						self.name().fg(state.parser.colors().info)
					))
				);
			}
		});

		reports
	}

	fn on_end<'a>(
		&self,
		location: Token,
		state: &ParserState,
		document: &'a dyn Document<'a>,
	) -> Vec<Report> {
		let kernel: Ref<'_, Kernel> =
			Ref::map(state.shared.kernels.borrow(), |b| b.get("main").unwrap());
		let ctx = KernelContext {
			location: location.clone(),
			state,
			document,
		};

		let mut reports = vec![];
		kernel.run_with_context(ctx, |lua| {
			let chunk = lua.load(self.end.as_str());
			if let Err(err) = chunk.eval::<()>() {
				report_err!(
					&mut reports,
					location.source(),
					"Lua execution failed".into(),
					span(location.range.clone(), err.to_string()),
					note(format!(
						"When trying to end custom style {}",
						self.name().fg(state.parser.colors().info)
					))
				);
			}
		});

		reports
	}
}

struct CustomStyleState {
	toggled: HashMap<String, Token>,
}

impl RuleState for CustomStyleState {
	fn scope(&self) -> Scope { Scope::PARAGRAPH }

	fn on_remove(&self, state: &ParserState, document: &dyn Document) -> Vec<Report> {
		let mut reports = vec![];

		self.toggled.iter().for_each(|(style, token)| {
			let paragraph = document.last_element::<Paragraph>().unwrap();
			let paragraph_end = paragraph
				.content
				.last()
				.map(|last| {
					(
						last.location().source(),
						last.location().end() - 1..last.location().end(),
					)
				})
				.unwrap();

			report_err!(
				&mut reports,
				token.source(),
				"Unterminated Custom Style".into(),
				span(
					token.range.clone(),
					format!("Style {} starts here", style.fg(state.parser.colors().info))
				),
				span(paragraph_end.1, "Paragraph ends here".into()),
				note("Styles cannot span multiple documents (i.e @import)".into())
			);
		});

		reports
	}
}

static STATE_NAME: &str = "elements.custom_style";

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::customstyle")]
pub struct CustomStyleRule;

impl CustomStyleRule {
	pub fn new() -> Self { Self {} }
}

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

		(cursor.at(token.end()), unsafe {
			std::mem::transmute(reports)
		})
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];

		bindings.push((
			"define_toggled".into(),
			lua.create_function(
				|_, (name, token, on_start, on_end): (String, String, String, String)| {
					let mut result = Ok(());

					let style = LuaCustomStyle {
						tokens: CustomStyleToken::Toggle(token),
						name: name.clone(),
						start: on_start,
						end: on_end,
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
					String,
					String,
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
						start: on_start,
						end: on_end,
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

#[cfg(test)]
mod tests {
	use crate::elements::raw::Raw;
	use crate::elements::text::Text;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::ParseMode;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;

	use super::*;

	#[test]
	fn toggle() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
%<[main]
function my_style_start()
	nml.raw.push("inline", "start")
end
function my_style_end()
	nml.raw.push("inline", "end")
end
function red_style_start()
	nml.raw.push("inline", "<a style=\"color:red\">")
end
function red_style_end()
	nml.raw.push("inline", "</a>")
end
nml.custom_style.define_toggled("My Style", "|", "my_style_start()", "my_style_end()")
nml.custom_style.define_toggled("My Style2", "°", "red_style_start()", "red_style_end()")
>%
pre |styled| post °Hello°.
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
			Paragraph {
				Text { content == "pre " };
				Raw { content == "start" };
				Text { content == "styled" };
				Raw { content == "end" };
				Text { content == " post " };
				Raw { content == "<a style=\"color:red\">" };
				Text { content == "Hello" };
				Raw { content == "</a>" };
				Text { content == "." };
			};
		);
	}

	#[test]
	fn paired() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
%<[main]
function my_style_start()
	nml.raw.push("inline", "start")
end
function my_style_end()
	nml.raw.push("inline", "end")
end
function red_style_start()
	nml.raw.push("inline", "<a style=\"color:red\">")
end
function red_style_end()
	nml.raw.push("inline", "</a>")
end
nml.custom_style.define_paired("My Style", "[", "]", "my_style_start()", "my_style_end()")
nml.custom_style.define_paired("My Style2", "(", ")", "red_style_start()", "red_style_end()")
>%
pre [styled] post (Hello).
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
			Paragraph {
				Text { content == "pre " };
				Raw { content == "start" };
				Text { content == "styled" };
				Raw { content == "end" };
				Text { content == " post " };
				Raw { content == "<a style=\"color:red\">" };
				Text { content == "Hello" };
				Raw { content == "</a>" };
				Text { content == "." };
			};
		);
	}
}
