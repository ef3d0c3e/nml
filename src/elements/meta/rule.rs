use std::any::Any;

use mlua::Lua;

use crate::add_documented_function;
use crate::lua::elem::ElemWrapper;
use crate::lua::kernel::Kernel;
use crate::lua::scope::ScopeWrapper;
use crate::parser::rule::Rule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::Cursor;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

#[auto_registry::auto_registry(registry = "rules")]
#[derive(Default)]
pub struct MetaRule;

impl Rule for MetaRule {
	fn name(&self) -> &'static str {
		"Meta"
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

	fn on_match<'u>(
		&self,
		_unit: &mut TranslationUnit,
		_cursor: &Cursor,
		_match_data: Box<dyn Any + Send + Sync>,
	) -> Cursor {
		panic!("Meta rule cannot match")
	}

	fn register_bindings(&self) {
		add_documented_function!(
			"scope.Scope",
			|lua: &Lua, (elems,): (Vec<ElemWrapper>,)| {
				Kernel::with_context(lua, |ctx| {
					ctx.unit.with_child(
						ctx.location.source(),
						ParseMode::default(),
						false,
						|unit, scope| {
							for elem in elems {
								unit.add_content(elem.0);
							}
							Ok(ScopeWrapper(scope))
						},
					)
				})
			},
			"Creates a new scope with content",
			vec!["elems:Element[] Elements that will populate the newly created scope"],
			"Scope The created scope"
		);
	}
}
