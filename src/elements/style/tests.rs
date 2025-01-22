use std::sync::Arc;

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
Some *style
terminated here*

**BOLD + *italic***
__`UNDERLINE+EM`__
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
		Paragraph {
			Text;
			Style { kind == 1, close == false };
			Text;
			Style { kind == 1, close == true };
		};
		Paragraph {
			Style { kind == 0, close == false }; // **
			Text;
			Style { kind == 1, close == false }; // *
			Text;
			Style { kind == 0, close == true }; // **
			Style { kind == 1, close == true }; // *

			Style { kind == 2, close == false }; // __
			Style { kind == 3, close == false }; // `
			Text;
			Style { kind == 3, close == true }; // `
			Style { kind == 2, close == true }; // __
		};
	);
}

#[test]
fn lua() {
	let source = Arc::new(SourceFile::with_content(
			"".to_string(),
			r#"
Some %<nml.style.toggle("italic")>%style
terminated here%<nml.style.toggle("Italic")>%

%<nml.style.toggle("Bold")>%NOLD + %<nml.style.toggle("italic")>%italic%<nml.style.toggle("bold") nml.style.toggle("italic")>%
%<nml.style.toggle("Underline") nml.style.toggle("Emphasis")>%UNDERLINE+EM%<nml.style.toggle("emphasis")>%%<nml.style.toggle("underline")>%
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
		Paragraph {
			Text;
			Style { kind == 1, close == false };
			Text;
			Style { kind == 1, close == true };
		};
		Paragraph {
			Style { kind == 0, close == false }; // **
			Text;
			Style { kind == 1, close == false }; // *
			Text;
			Style { kind == 0, close == true }; // **
			Style { kind == 1, close == true }; // *

			Style { kind == 2, close == false }; // __
			Style { kind == 3, close == false }; // `
			Text;
			Style { kind == 3, close == true }; // `
			Style { kind == 2, close == true }; // __
		};
	);
}

#[test]
fn semantic() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
**te📫st** `another`
__teかst__ *another*
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
	style_marker { delta_line == 1, delta_start == 0, length == 2 };
	style_marker { delta_line == 0, delta_start == 6 + '📫'.len_utf16() as u32, length == 2 };
	style_marker { delta_line == 0, delta_start == 3, length == 1 };
	style_marker { delta_line == 0, delta_start == 8, length == 1 };

	style_marker { delta_line == 1, delta_start == 0, length == 2 };
	style_marker { delta_line == 0, delta_start == 6 + 'か'.len_utf16() as u32, length == 2 };
	style_marker { delta_line == 0, delta_start == 3, length == 1 };
	style_marker { delta_line == 0, delta_start == 8, length == 1 };
	);
}
