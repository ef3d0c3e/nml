use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
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

use crate::document::customstyle::CustomStyle;
use crate::document::customstyle::CustomStyleToken;
use crate::document::document::Document;
use crate::document::document::DocumentAccessors;
use crate::lua::kernel::function_with_context;
use crate::lua::kernel::KernelContext;
use crate::lua::kernel::CTX;
use crate::parser::parser::Parser;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::state::Scope;
use crate::parser::state::State;

use lazy_static::lazy_static;

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
		parser: &dyn Parser,
		document: &'a dyn Document<'a>,
	) -> Result<(), Report<(Rc<dyn Source>, Range<usize>)>> {
		let kernel = parser.get_kernel("main").unwrap();
		let ctx = KernelContext {
			location: location.clone(),
			parser,
			document,
		};

		let mut result = Ok(());
		kernel.run_with_context(ctx, |lua| {
			let chunk = lua.load(self.start.as_str());
			if let Err(err) = chunk.eval::<()>() {
				result = Err(
					Report::build(ReportKind::Error, location.source(), location.start())
						.with_message("Lua execution failed")
						.with_label(
							Label::new((location.source(), location.range.clone()))
								.with_message(err.to_string())
								.with_color(parser.colors().error),
						)
						.with_note(format!(
							"When trying to start custom style {}",
							self.name().fg(parser.colors().info)
						))
						.finish(),
				);
			}
		});

		result
	}

	fn on_end<'a>(
		&self,
		location: Token,
		parser: &dyn Parser,
		document: &'a dyn Document<'a>,
	) -> Result<(), Report<(Rc<dyn Source>, Range<usize>)>> {
		let kernel = parser.get_kernel("main").unwrap();
		let ctx = KernelContext {
			location: location.clone(),
			parser,
			document,
		};

		let mut result = Ok(());
		kernel.run_with_context(ctx, |lua| {
			let chunk = lua.load(self.end.as_str());
			if let Err(err) = chunk.eval::<()>() {
				result = Err(
					Report::build(ReportKind::Error, location.source(), location.start())
						.with_message("Lua execution failed")
						.with_label(
							Label::new((location.source(), location.range.clone()))
								.with_message(err.to_string())
								.with_color(parser.colors().error),
						)
						.with_note(format!(
							"When trying to end custom style {}",
							self.name().fg(parser.colors().info)
						))
						.finish(),
				);
			}
		});

		result
	}
}

struct CustomStyleState {
	toggled: HashMap<String, Token>,
}

impl State for CustomStyleState {
	fn scope(&self) -> Scope { Scope::PARAGRAPH }

	fn on_remove<'a>(
		&self,
		parser: &dyn Parser,
		document: &dyn Document,
	) -> Vec<Report<'a, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		self.toggled.iter().for_each(|(style, token)| {
			let paragraph = document.last_element::<Paragraph>().unwrap();
			let paragraph_end = paragraph
				.content
				.last()
				.and_then(|last| {
					Some((
						last.location().source(),
						last.location().end() - 1..last.location().end(),
					))
				})
				.unwrap();

			reports.push(
				Report::build(ReportKind::Error, token.source(), token.start())
					.with_message("Unterminated Custom Style")
					.with_label(
						Label::new((token.source(), token.range.clone()))
							.with_order(1)
							.with_message(format!(
								"Style {} starts here",
								style.fg(parser.colors().info)
							))
							.with_color(parser.colors().error),
					)
					.with_label(
						Label::new(paragraph_end)
							.with_order(1)
							.with_message(format!("Paragraph ends here"))
							.with_color(parser.colors().error),
					)
					.with_note("Styles cannot span multiple documents (i.e @import)")
					.finish(),
			);
		});

		return reports;
	}
}

pub struct CustomStyleRule;

lazy_static! {
	static ref STATE_NAME: String = "elements.custom_style".to_string();
}

impl Rule for CustomStyleRule {
	fn name(&self) -> &'static str { "Custom Style" }

	fn next_match(&self, parser: &dyn Parser, cursor: &Cursor) -> Option<(usize, Box<dyn Any>)> {
		let content = cursor.source.content();

		let mut closest_match = usize::MAX;
		let mut matched_style = (None, false);
		parser
			.custom_styles()
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
		parser: &dyn Parser,
		document: &'a dyn Document<'a>,
		cursor: Cursor,
		match_data: Option<Box<dyn Any>>,
	) -> (Cursor, Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>) {
		let (style, end) = match_data
			.as_ref()
			.unwrap()
			.downcast_ref::<(Rc<dyn CustomStyle>, bool)>()
			.unwrap();

		let query = parser.state().query(&STATE_NAME);
		let state = match query {
			Some(state) => state,
			None => {
				// Insert as a new state
				match parser.state_mut().insert(
					STATE_NAME.clone(),
					Rc::new(RefCell::new(CustomStyleState {
						toggled: HashMap::new(),
					})),
				) {
					Err(_) => panic!("Unknown error"),
					Ok(state) => state,
				}
			}
		};

		let (close, token) = match style.tokens() {
			CustomStyleToken::Toggle(s) => {
				let mut borrow = state.borrow_mut();
				let state = borrow.downcast_mut::<CustomStyleState>().unwrap();

				match state.toggled.get(style.name()) {
					Some(_) => {
						// Terminate style
						let token =
							Token::new(cursor.pos..cursor.pos + s.len(), cursor.source.clone());

						state.toggled.remove(style.name());
						(true, token)
					}
					None => {
						// Start style
						let token =
							Token::new(cursor.pos..cursor.pos + s.len(), cursor.source.clone());

						state.toggled.insert(style.name().into(), token.clone());
						(false, token)
					}
				}
			}
			CustomStyleToken::Pair(s_begin, s_end) => {
				let mut borrow = state.borrow_mut();
				let state = borrow.downcast_mut::<CustomStyleState>().unwrap();

				if *end {
					// Terminate style
					let token =
						Token::new(cursor.pos..cursor.pos + s_end.len(), cursor.source.clone());
					if state.toggled.get(style.name()).is_none() {
						return (
							cursor.at(cursor.pos + s_end.len()),
							vec![
								Report::build(ReportKind::Error, token.source(), token.start())
									.with_message("Invalid End of Style")
									.with_label(
										Label::new((token.source(), token.range.clone()))
											.with_order(1)
											.with_message(format!(
											"Cannot end style {} here, is it not started anywhere",
											style.name().fg(parser.colors().info)
										))
											.with_color(parser.colors().error),
									)
									.finish(),
							],
						);
					}

					state.toggled.remove(style.name());
					(true, token)
				} else {
					// Start style
					let token = Token::new(
						cursor.pos..cursor.pos + s_begin.len(),
						cursor.source.clone(),
					);
					if let Some(start_token) = state.toggled.get(style.name()) {
						return (
							cursor.at(cursor.pos + s_end.len()),
							vec![Report::build(
								ReportKind::Error,
								start_token.source(),
								start_token.start(),
							)
							.with_message("Invalid Start of Style")
							.with_label(
								Label::new((token.source(), token.range.clone()))
									.with_order(1)
									.with_message(format!(
										"Style cannot {} starts here",
										style.name().fg(parser.colors().info)
									))
									.with_color(parser.colors().error),
							)
							.with_label(
								Label::new((start_token.source(), start_token.range.clone()))
									.with_order(2)
									.with_message(format!(
										"Style {} starts previously here",
										style.name().fg(parser.colors().info)
									))
									.with_color(parser.colors().error),
							)
							.finish()],
						);
					}

					state.toggled.insert(style.name().into(), token.clone());
					(false, token)
				}
			}
		};

		if let Err(rep) = if close {
			style.on_end(token.clone(), parser, document)
		} else {
			style.on_start(token.clone(), parser, document)
		} {
			return (
				cursor.at(token.end()),
				vec![unsafe {
					// TODO
					std::mem::transmute(rep)
				}],
			);
		} else {
			(cursor.at(token.end()), vec![])
		}
	}

	fn lua_bindings<'lua>(&self, lua: &'lua Lua) -> Option<Vec<(String, Function<'lua>)>> {
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
							if let Some(_) = ctx.parser.get_custom_style(name.as_str()) {
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
							ctx.parser.insert_custom_style(Rc::new(style));
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

					let style = LuaCustomStyle {
						tokens: CustomStyleToken::Pair(token_start, token_end),
						name: name.clone(),
						start: on_start,
						end: on_end,
					};

					CTX.with_borrow(|ctx| {
						ctx.as_ref().map(|ctx| {
							if let Some(_) = ctx.parser.get_custom_style(name.as_str()) {
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
							ctx.parser.insert_custom_style(Rc::new(style));
						});
					});

					result
				},
			)
			.unwrap(),
		));

		Some(bindings)
	}
}

#[cfg(test)]
mod tests {
	use crate::elements::raw::Raw;
	use crate::elements::text::Text;
	use crate::parser::langparser::LangParser;
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
		let doc = parser.parse(source, None);

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
		let doc = parser.parse(source, None);

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
