use std::sync::Arc;

use mlua::LuaSerdeExt;
use mlua::UserData;

use crate::add_documented_method;
use crate::elements::variable::elem::VariableSubstitution;
use crate::unit::translation::TranslationAccessors;
use crate::unit::variable::Variable;

use super::kernel::Kernel;

#[auto_registry::auto_registry(registry = "lua")]
pub struct VariableWrapper(pub Arc<dyn Variable>);

impl UserData for VariableWrapper {
	fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(_fields: &mut F) {}

	fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
		add_documented_method!(
			methods,
			"Variable",
			"location",
			|_lua, this, ()| { Ok(this.0.location().clone()) },
			"Returns the location where the variable is defined",
			vec!["self"],
			Some("Token")
		);
		add_documented_method!(
			methods,
			"Variable",
			"typename",
			|lua, this, ()| {
				lua.to_value(this.0.variable_typename())
			},
			"Returns the name of this variable's type",
			vec!["self"],
			Some("string")
		);
		add_documented_method!(
			methods,
			"Variable",
			"name",
			|lua, this, ()| {
				lua.to_value(&this.0.name().0)
			},
			"Returns the name of this variable",
			vec!["self"],
			Some("string")
		);
		add_documented_method!(
			methods,
			"Variable",
			"value_token",
			|_lua, this, ()| {
				Ok(this.0.value_token().clone())
			},
			"Returns the token of this variable's value",
			vec!["self"],
			Some("Token")
		);
		add_documented_method!(
			methods,
			"Variable",
			"expand",
			|lua, this, ()| {
				Kernel::with_context(lua, |ctx| {
					let result = this.0.expand(ctx.unit, ctx.location.clone());

					ctx.unit.add_content(Arc::new(VariableSubstitution {
						location: ctx.location.clone(),
						variable: this.0.clone(),
						content: vec![result],
					}));
				});
				Ok(())
			},
			"Expands the variable in place",
			vec!["self"],
			None
		);
		add_documented_method!(
			methods,
			"Variable",
			"to_string",
			|lua, this, ()| {
				lua.to_value(&this.0.to_string())
			},
			"Converts this variable's content to a string",
			vec!["self"],
			Some("string")
		);
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
