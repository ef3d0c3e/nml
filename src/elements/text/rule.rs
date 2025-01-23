use std::any::Any;
use std::sync::Arc;

use mlua::Function;
use mlua::Lua;

use crate::document::document::Document;
use crate::lua::kernel::Kernel;
use crate::lua::kernel::CTX;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::reports::Report;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::translation::TranslationAccessors;
use crate::parser::translation::TranslationUnit;

use super::elem::Text;

#[auto_registry::auto_registry(registry = "rules")]
#[derive(Default)]
pub struct TextRule;

impl Rule for TextRule {
	fn name(&self) -> &'static str { "Text" }

	fn previous(&self) -> Option<&'static str> { Some("Link") }

	fn next_match(
		&self,
		_mode: &ParseMode,
		_cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any>)> {
		None
	}

	fn on_match(
		&self,
		_unit: &mut TranslationUnit,
		_cursor: &Cursor,
		_match_data: Box<dyn Any>,
	) -> (Cursor, Vec<Report>) {
		panic!("Text cannot match");
	}

	fn register_bindings<'lua>(&self, kernel: &'lua Kernel) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];
		bindings.push((
			"push".to_string(),
			kernel.lua.create_function(|_, content: String| {
				kernel.with_context(|mut ctx| {
					ctx.unit.add_content(Arc::new(Text {
						location: ctx.location.clone(),
						content
					}));
					ctx
				})
			})
			.unwrap(),
		));

		bindings
	}
}
