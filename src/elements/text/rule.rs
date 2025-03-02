use std::any::Any;
use std::rc::Rc;

use crate::lua::kernel::Kernel;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::state::ParseMode;
use crate::unit::translation::{TranslationAccessors, TranslationUnit};

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
	) -> Cursor {
		panic!("Text cannot match");
	}

	fn register_bindings(&self, kernel: &Kernel, table: mlua::Table) {
		kernel.create_function(table.clone(), "push", |mut ctx, _, content: String| {
			let location = ctx.location.clone();
			ctx.unit.add_content(Rc::new(Text {
				location,
				content
			}));
			Ok(())
		});
	}
}
