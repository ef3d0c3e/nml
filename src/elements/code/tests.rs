use std::sync::Arc;

use crate::elements::code::elem::Code;
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
int foo;```
```C
int foo;```
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
	);
}
