use std::sync::Arc;


use mlua::Scope;

use crate::elements::heading::elem::Heading;
use crate::elements::meta::scope::ScopeElement;
use crate::elements::raw::elem::Raw;
use crate::elements::style::elem::StyleElem;
use crate::elements::text::elem::Text;
use crate::parser::parser::Parser;
use crate::parser::source::SourceFile;
use crate::unit::element::ElemKind;
use crate::unit::translation::TranslationUnit;
use crate::validate_ast;

#[test]
fn parser() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
# H1A **bold**
## H2A
# H1B
###{ref} H3B
"#.to_string(),
		None,
	));

	let parser = Parser::new();
	let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
	let (reports, unit) = unit.consume("".into());
	assert!(reports.is_empty());

	validate_ast!(unit.get_entry_scope(), 0,
		Heading { depth == 1 } [
			{
				Text { content == "H1A " };
				StyleElem { enable == true };
				Text { content == "bold" };
				StyleElem { enable == false };
			}
		];
		Heading { depth == 2 } [
			{
				Text { content == "H2A" };
			}
		];
		Heading { depth == 1 } [
			{
				Text { content == "H1B" };
			}
		];
		Heading { depth == 3 } [
			{
				Text { content == "H3B" };
			}
		];
	);
}


#[test]
fn lua() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"{:lua 
local sc = nml.scope.Scope({nml.text.Text("Test")})
nml.unit():add_content(nml.heading.Heading(1, true, sc))
local sc = nml.scope.Scope({nml.text.Text("Foo bar")})
nml.unit():add_content(nml.heading.Heading(5, false, sc, "ref"))
:}
"#.to_string(),
		None,
	));

	let parser = Parser::new();
	let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
	let (reports, unit) = unit.consume("".into());
	assert!(reports.is_empty());

	validate_ast!(unit.get_entry_scope(), 0,
		ScopeElement [
			{
				Heading { depth == 1 } [ {
					Text { content == "Test" };
				} ];
				Heading { depth == 5 } [ {
					Text { content == "Foo bar" };
				} ];
			}
		];
	);
}


