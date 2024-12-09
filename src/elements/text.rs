use std::any::Any;

use mlua::Function;
use mlua::Lua;

use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::lua::kernel::CTX;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::reports::*;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct Text {
	pub location: Token,
	pub content: String,
}

impl Text {
	pub fn new(location: Token, content: String) -> Text { Text { location, content } }
}

impl Element for Text {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Inline }
	fn element_name(&self) -> &'static str { "Text" }

	fn compile(
		&self,
		compiler: &Compiler,
		_document: &dyn Document,
		_cursor: usize,
	) -> Result<String, String> {
		Ok(Compiler::sanitize(compiler.target(), self.content.as_str()))
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::text")]
#[derive(Default)]
pub struct TextRule;

impl Rule for TextRule {
	fn name(&self) -> &'static str { "Text" }

	fn previous(&self) -> Option<&'static str> { Some("Link") }

	fn next_match(
		&self,
		_mode: &ParseMode,
		_state: &ParserState,
		_cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any>)> {
		None
	}

	fn on_match(
		&self,
		_state: &ParserState,
		_document: &dyn Document,
		_cursor: Cursor,
		_match_data: Box<dyn Any>,
	) -> (Cursor, Vec<Report>) {
		panic!("Text cannot match");
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];
		bindings.push((
			"push".to_string(),
			lua.create_function(|_, content: String| {
				CTX.with_borrow(|ctx| {
					ctx.as_ref().map(|ctx| {
						ctx.state.push(
							ctx.document,
							Box::new(Text {
								location: ctx.location.clone(),
								content,
							}),
						);
					})
				});

				Ok(())
			})
			.unwrap(),
		));

		bindings
	}
}
