use std::sync::Arc;

use mlua::LuaSerdeExt;
use mlua::UserData;

use crate::elements::variable::elem::VariableSubstitution;
use crate::unit::translation::TranslationAccessors;
use crate::unit::variable::Variable;

use super::kernel::Kernel;

pub struct VariableWrapper {
	pub inner: Arc<dyn Variable>,
}

impl UserData for VariableWrapper {
	fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(_fields: &mut F) {}

	fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
		methods.add_method("location", |_lua, this, ()| {
			Ok(this.inner.location().clone())
		});
		methods.add_method("typename", |lua, this, ()| {
			lua.to_value(this.inner.variable_typename())
		});
		methods.add_method("name", |lua, this, ()| lua.to_value(&this.inner.name().0));
		methods.add_method("value_token", |_lua, this, ()| {
			Ok(this.inner.value_token().clone())
		});
		methods.add_method("expand", |lua, this, ()| {
			Kernel::with_context(lua, |ctx| {
				let result = this.inner.expand(ctx.unit, ctx.location.clone());

				ctx.unit.add_content(Arc::new(VariableSubstitution {
					location: ctx.location.clone(),
					variable: this.inner.clone(),
					content: vec![result],
				}));
			});
			Ok(())
		});
		methods.add_method("to_string", |lua, this, ()| {
			lua.to_value(&this.inner.to_string())
		});
	}
}

#[cfg(test)]
mod test {
	use crate::elements::meta::scope::ScopeElement;
	use crate::elements::style::elem::StyleElem;
	use crate::elements::text::elem::Text;
	use crate::elements::variable::elem::VariableDefinition;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::unit::translation::TranslationUnit;
	use crate::validate_ast;

	use super::*;

	#[test]
	fn test() {
		let source = Arc::new(SourceFile::with_content(
			"".to_string(),
			r#":set var = {{**bold**}}
{:lua nml.unit():get_variable("var"):expand():}
{:lua' nml.unit():get_variable("var"):to_string():}"#
				.to_string(),
			None,
		));
		let parser = Parser::new();
		let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
		let (reports, unit) = unit.consume("".into());
		assert!(reports.is_empty());

		validate_ast!(unit.get_entry_scope(), 0,
			VariableDefinition;
			ScopeElement [{
					VariableSubstitution [{
						StyleElem { enable == true };
						Text { content == "bold" };
						StyleElem { enable == false };
					}];
			}];
			ScopeElement [{
				Text { content == "**bold**" };
			}];
		);
	}
}
