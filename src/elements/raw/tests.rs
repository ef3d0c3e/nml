use std::rc::Rc;

use crate::document::element::ElemKind;
use crate::elements::paragraph::elem::Paragraph;
use crate::elements::raw::elem::Raw;
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
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
Break{?[kind=block] Raw?}NewParagraph{?<b>?}
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
		Raw { kind == ElemKind::Block, content == "Raw" };
		Paragraph {
			Text;
			Raw { kind == ElemKind::Inline, content == "<b>" };
		};
	);
}

#[test]
fn lua() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
Break%<nml.raw.push("block", "Raw")>%NewParagraph%<nml.raw.push("inline", "<b>")>%
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
	Raw { kind == ElemKind::Block, content == "Raw" };
	Paragraph {
		Text;
		Raw { kind == ElemKind::Inline, content == "<b>" };
	};
	);
}

#[test]
fn semantic() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
{?[kind=block] Raw?}
{?<b>?}
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
		raw_sep { delta_line == 1, delta_start == 0, length == 2 };
		raw_props_sep { delta_line == 0, delta_start == 2, length == 1 };
		prop_name { delta_line == 0, delta_start == 1, length == 4 };
		prop_equal { delta_line == 0, delta_start == 4, length == 1 };
		prop_value { delta_line == 0, delta_start == 1, length == 5 };
		raw_props_sep { delta_line == 0, delta_start == 5, length == 1 };
		raw_content { delta_line == 0, delta_start == 1, length == 4 };
		raw_sep { delta_line == 0, delta_start == 4, length == 2 };
		raw_sep { delta_line == 1, delta_start == 0, length == 2 };
		raw_content { delta_line == 0, delta_start == 2, length == 3 };
		raw_sep { delta_line == 0, delta_start == 3, length == 2 };
	);
}
