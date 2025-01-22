use std::sync::Arc;

use crate::elements::link::elem::Link;
use crate::elements::list::elem::ListEntry;
use crate::elements::list::elem::ListMarker;
use crate::elements::paragraph::elem::Paragraph;
use crate::elements::style::elem::Style;
use crate::elements::text::elem::Text;
use crate::parser::langparser::LangParser;
use crate::parser::parser::ParseMode;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::source::SourceFile;
use crate::validate_document;
use crate::validate_semantics;

#[test]
fn parser() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
Simple evals:
 * 1+1: %< 1+1>%
 * %<" 1+1>% = 2
 * %<! "**bold**">%

Definition:
@<
function make_ref(name, ref)
	return "[" .. name .. "](#" .. ref .. ")"
end
>@
Evaluation: %<! make_ref("hello", "id")>%
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

	validate_document!(doc.content().borrow(), 0,
		Paragraph;
		ListMarker;
		ListEntry;
		ListEntry {
			Text { content == "2" };
			Text { content == " = 2" };
		};
		ListEntry {
			Style;
			Text { content == "bold" };
			Style;
		};
		ListMarker;
		Paragraph {
			Text; Text;
			Link { url == "#id" } { Text { content == "hello" }; };
		};
	);
}

#[test]
fn semantic() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
%<[test]! "Hello World">%
@<main
function add(x, y)
	return x + y
end
>@
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
		script_sep { delta_line == 1, delta_start == 0, length == 2 };
		script_kernel_sep { delta_line == 0, delta_start == 2, length == 1 };
		script_kernel { delta_line == 0, delta_start == 1, length == 4 };
		script_kernel_sep { delta_line == 0, delta_start == 4, length == 1 };
		script_kind { delta_line == 0, delta_start == 1, length == 1 };
		script_sep { delta_line == 0, delta_start == 15, length == 2 };

		script_sep { delta_line == 1, delta_start == 0, length == 2 };
		script_kernel { delta_line == 0, delta_start == 2, length == 4 };
		script_sep { delta_line == 4, delta_start == 0, length == 2 };
	);
}
