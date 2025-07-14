use std::sync::Arc;

use crate::elements::variable::elem::VariableDefinition;
use crate::parser::parser::Parser;
use crate::parser::source::SourceFile;
use crate::unit::translation::TranslationUnit;
use crate::unit::variable::{VariableName, VariableVisibility};
use crate::validate_ast;

#[test]
fn parser() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#":set var = 'foo'
:export val = "bar"
:set multi = '''baz
multi

line'''
:set content = {{
TEXT *italic*
}}
"#.to_string(),
		None,
	));

	let parser = Parser::new();
	let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
	let (reports, unit) = unit.consume("".into());
	assert!(reports.is_empty());

	validate_ast!(unit.get_entry_scope(), 0,
		VariableDefinition {
			variable.name() == &VariableName("var".into()),
			variable.visibility() == &VariableVisibility::Internal,
			variable.to_string() == "foo",
			variable.variable_typename() == "property",
		};
		VariableDefinition {
			variable.name() == &VariableName("val".into()),
			variable.visibility() == &VariableVisibility::Exported,
			variable.to_string() == "bar",
			variable.variable_typename() == "property",
		};
		VariableDefinition {
			variable.name() == &VariableName("multi".into()),
			variable.visibility() == &VariableVisibility::Internal,
			variable.to_string() == "baz\nmulti\n\nline",
			variable.variable_typename() == "property",
		};
		VariableDefinition {
			variable.name() == &VariableName("content".into()),
			variable.visibility() == &VariableVisibility::Internal,
			variable.to_string() == "\nTEXT *italic*\n",
			variable.variable_typename() == "content",
		};
	);
}

/*
#[test]
fn lua() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"TEXT
{:lua for scope, elem in nml.unit():content(true) do
	nml.text.push(elem:downcast().content)
	nml.text.push("Lua")
end:}"#
			.to_string(),
		None,
	));
	let parser = Parser::new();
	let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
	let (reports, unit) = unit.consume("".into());
	assert!(reports.is_empty());

	validate_ast!(unit.get_entry_scope(), 0,
		Text { content == "TEXT" };
		ScopeElement [{
			Text { content == "TEXT" };
			Text { content == "Lua" };
		}];
	);
}
*/
