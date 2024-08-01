use std::any::Any;
use std::ops::Range;
use std::rc::Rc;

use ariadne::Report;
use mlua::Function;
use mlua::Lua;

use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::lua::kernel::CTX;
use crate::parser::parser::Parser;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::source::Source;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct Text {
	pub location: Token,
	pub content: String,
}

impl Text {
	pub fn new(location: Token, content: String) -> Text {
		Text {
			location: location,
			content: content,
		}
	}
}

impl Element for Text {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Inline }
	fn element_name(&self) -> &'static str { "Text" }
	fn to_string(&self) -> String { format!("{self:#?}") }

	fn compile(&self, compiler: &Compiler, _document: &dyn Document) -> Result<String, String> {
		Ok(Compiler::sanitize(compiler.target(), self.content.as_str()))
	}
}

#[derive(Default)]
pub struct TextRule;

impl Rule for TextRule {
	fn name(&self) -> &'static str { "Text" }

	fn next_match(&self, _cursor: &Cursor) -> Option<(usize, Box<dyn Any>)> { None }

	fn on_match(
		&self,
		_parser: &dyn Parser,
		_document: &dyn Document,
		_cursor: Cursor,
		_match_data: Option<Box<dyn Any>>,
	) -> (Cursor, Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>) {
		panic!("Text cannot match");
	}

	fn lua_bindings<'lua>(&self, lua: &'lua Lua) -> Option<Vec<(String, Function<'lua>)>> {
		let mut bindings = vec![];
		bindings.push((
			"push".to_string(),
			lua.create_function(|_, content: String| {
				CTX.with_borrow(|ctx| {
					ctx.as_ref().map(|ctx| {
						ctx.parser.push(
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

		Some(bindings)
	}
}
