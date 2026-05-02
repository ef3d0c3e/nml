use std::sync::Arc;

use crate::elements::code::elem::Code;
use crate::elements::linebreak::elem::LineBreak;
use crate::elements::meta::eof::Eof;
use crate::elements::meta::scope::ScopeElement;
use crate::parser::parser::Parser;
use crate::parser::source::SourceFile;
use crate::unit::translation::TranslationUnit;
use crate::validate_ast;

#[test]
fn parser() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"```C, Code
int foo;```
```[line_offset=7]C, Code
int foo;```
```C,
int foo```
```C
int foo```
"#.to_string(),
		None,
	));

	let parser = Parser::new();
	let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
	let (reports, unit) = unit.consume("".into());
	assert!(reports.is_empty());

	validate_ast!(unit.get_entry_scope(), 0,
		Code {
			language == "C",
			display.title == Some("Code".to_string())
		};
		Code {
			language == "C",
			display.title == Some("Code".to_string()),
			display.line_offset == 7
		};
		Code {
			language == "C",
			display.title == None,
		};
		Code {
			language == "C",
			display.title == None,
		};
	);
}

#[test]
fn lua()
{
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"{:lua
local display = {
	title = "foo bar",
	line_gutter = true,
	line_offset = 32,
	inline = false,
	max_lines = 64,
	theme = "dark"
}
nml.unit():add_content(nml.code.Code(display, "C++", "int main() { return 0; }")):}
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
				Code {
					language == "C++",
					display.title == Some("foo bar".to_string()),
					display.line_gutter == true,
					display.line_offset == 32,
					display.inline == false,
					display.max_lines == Some(64),
					display.theme == Some("dark".to_string()),
				};
			}
		];
		LineBreak;
		Eof;
	);
}
