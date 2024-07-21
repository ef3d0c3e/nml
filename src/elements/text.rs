use mlua::{Function, Lua};

use crate::{compiler::compiler::Compiler, document::{document::Document, element::{ElemKind, Element}}, lua::kernel::CTX, parser::{rule::Rule, source::Token}};

#[derive(Debug)]
pub struct Text
{
    pub(self) location: Token,
	pub(self) content: String,
}

impl Text
{
	pub fn new(location: Token, content: String) -> Text
	{
		Text {
            location: location,
			content: content
		}
	}
}

impl Element for Text
{
    fn location(&self) -> &Token { &self.location }
    fn kind(&self) -> ElemKind { ElemKind::Inline }
	fn element_name(&self) -> &'static str { "Text" }
    fn to_string(&self) -> String { format!("{self:#?}") }

    fn compile(&self, compiler: &Compiler, _document: &Document) -> Result<String, String> {
        Ok(compiler.sanitize(self.content.as_str()))
    }
}

#[derive(Default)]
pub struct TextRule;

impl Rule for TextRule
{
    fn name(&self) -> &'static str { "Text" }

    fn next_match(&self, cursor: &crate::parser::source::Cursor) -> Option<(usize, Box<dyn std::any::Any>)> { None }

    fn on_match(&self, parser: &dyn crate::parser::parser::Parser, document: &crate::document::document::Document, cursor: crate::parser::source::Cursor, match_data: Option<Box<dyn std::any::Any>>) -> (crate::parser::source::Cursor, Vec<ariadne::Report<'_, (std::rc::Rc<dyn crate::parser::source::Source>, std::ops::Range<usize>)>>) { panic!("Text canno match"); }

    fn lua_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];

		bindings.push(("push".to_string(), lua.create_function(
			|_, content: String| {
			CTX.with_borrow(|ctx| ctx.as_ref().map(|ctx| {
				ctx.parser.push(ctx.document, Box::new(Text {
					location: ctx.location.clone(),
					content,
				}));
			}));

			Ok(())
		}).unwrap()));
		
		bindings
    }
}
