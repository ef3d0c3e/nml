use std::rc::Rc;

use crate::elements::code::elem::Code;
use crate::elements::code::elem::CodeKind;
use crate::parser::langparser::LangParser;
use crate::parser::parser::ParseMode;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::source::SourceFile;
use crate::validate_semantics;

#[test]
fn code_block() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
```[line_offset=32] C, Some Code...
static int INT32_MIN = 0x80000000;
```
%<nml.code.push_block("Lua", "From Lua", "print(\"Hello, World!\")", nil)>%
``Rust,
fn fact(n: usize) -> usize
{
	match n
	{
		0 | 1 => 1,
		_ => n * fact(n-1)
	}
}
``
%<nml.code.push_miniblock("Bash", "NUM=$(($RANDOM % 10))", 18)>%
"#
		.to_string(),
		None,
	));
	let parser = LangParser::default();
	let (doc, _) = parser.parse(
		ParserState::new(&parser, None),
		source,
		None,
		ParseMode::default(),
	);

	let borrow = doc.content().borrow();
	let found = borrow
		.iter()
		.filter_map(|e| e.downcast_ref::<Code>())
		.collect::<Vec<_>>();

	assert_eq!(found[0].block, CodeKind::FullBlock);
	assert_eq!(found[0].language, "C");
	assert_eq!(found[0].name, Some("Some Code...".to_string()));
	assert_eq!(found[0].code, "static int INT32_MIN = 0x80000000;");
	assert_eq!(found[0].line_offset, 32);

	assert_eq!(found[1].block, CodeKind::FullBlock);
	assert_eq!(found[1].language, "Lua");
	assert_eq!(found[1].name, Some("From Lua".to_string()));
	assert_eq!(found[1].code, "print(\"Hello, World!\")");
	assert_eq!(found[1].line_offset, 1);

	assert_eq!(found[2].block, CodeKind::MiniBlock);
	assert_eq!(found[2].language, "Rust");
	assert_eq!(found[2].name, None);
	assert_eq!(found[2].code, "\nfn fact(n: usize) -> usize\n{\n\tmatch n\n\t{\n\t\t0 | 1 => 1,\n\t\t_ => n * fact(n-1)\n\t}\n}");
	assert_eq!(found[2].line_offset, 1);

	assert_eq!(found[3].block, CodeKind::MiniBlock);
	assert_eq!(found[3].language, "Bash");
	assert_eq!(found[3].name, None);
	assert_eq!(found[3].code, "NUM=$(($RANDOM % 10))");
	assert_eq!(found[3].line_offset, 18);
}

#[test]
fn code_inline() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
``C, int fact(int n)``
``Plain Text, Text in a code block!``
%<nml.code.push_inline("C++", "std::vector<std::vector<int>> u;")>%
			"#
		.to_string(),
		None,
	));
	let parser = LangParser::default();
	let (doc, _) = parser.parse(
		ParserState::new(&parser, None),
		source,
		None,
		ParseMode::default(),
	);

	let borrow = doc.content().borrow();
	let found = borrow
		.first()
		.unwrap()
		.as_container()
		.unwrap()
		.contained()
		.iter()
		.filter_map(|e| e.downcast_ref::<Code>())
		.collect::<Vec<_>>();

	assert_eq!(found[0].block, CodeKind::Inline);
	assert_eq!(found[0].language, "C");
	assert_eq!(found[0].name, None);
	assert_eq!(found[0].code, "int fact(int n)");
	assert_eq!(found[0].line_offset, 1);

	assert_eq!(found[1].block, CodeKind::Inline);
	assert_eq!(found[1].language, "Plain Text");
	assert_eq!(found[1].name, None);
	assert_eq!(found[1].code, "Text in a code block!");
	assert_eq!(found[1].line_offset, 1);

	assert_eq!(found[2].block, CodeKind::Inline);
	assert_eq!(found[2].language, "C++");
	assert_eq!(found[2].name, None);
	assert_eq!(found[2].code, "std::vector<std::vector<int>> u;");
	assert_eq!(found[2].line_offset, 1);
}

#[test]
fn semantic() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
```[line_offset=15] C, Title
test code
```
``C, Single Line``
		"#
		.to_string(),
		None,
	));
	let parser = LangParser::default();
	let (_, state) = parser.parse(
		ParserState::new_with_semantics(&parser, None),
		source.clone(),
		None,
		ParseMode::default(),
	);
	validate_semantics!(state, source.clone(), 0,
	code_sep { delta_line == 1, delta_start == 0, length == 3 };
	code_props_sep { delta_line == 0, delta_start == 3, length == 1 };
	prop_name { delta_line == 0, delta_start == 1, length == 11 };
	prop_equal { delta_line == 0, delta_start == 11, length == 1 };
	prop_value { delta_line == 0, delta_start == 1, length == 2 };
	code_props_sep { delta_line == 0, delta_start == 2, length == 1 };
	code_lang { delta_line == 0, delta_start == 1, length == 2 };
	code_title { delta_line == 0, delta_start == 3, length == 6 };
	code_sep { delta_line == 2, delta_start == 0, length == 3 };

	code_sep { delta_line == 1, delta_start == 0, length == 2 };
	code_lang { delta_line == 0, delta_start == 2, length == 1 };
	code_sep { delta_line == 0, delta_start == 14, length == 2 };
	);
}
