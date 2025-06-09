use std::any::Any;
use std::sync::Arc;

use crate::lua::kernel::Kernel;
use crate::parser::rule::Rule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::Cursor;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

use super::elem::Text;

#[auto_registry::auto_registry(registry = "rules")]
#[derive(Default)]
pub struct TextRule;

impl Rule for TextRule {
	fn name(&self) -> &'static str {
		"Text"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Meta
	}

	fn next_match(
		&self,
		_unit: &TranslationUnit,
		_mode: &ParseMode,
		_states: &mut CustomStates,
		_cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any + Send + Sync>)> {
		None
	}

	fn on_match(
		&self,
		_unit: &mut TranslationUnit,
		_cursor: &Cursor,
		_match_data: Box<dyn Any + Send + Sync>,
	) -> Cursor {
		panic!("Text cannot match");
	}

	fn register_bindings(&self, kernel: &Kernel, table: mlua::Table) {
		kernel.create_function(table.clone(), "push", |lua, content: String| {
			Kernel::with_context(lua, |ctx| {
				ctx.unit.add_content(Arc::new(Text {
					location: ctx.location.clone(),
					content,
				}));
			});
			Ok(())
		});
	}
}
