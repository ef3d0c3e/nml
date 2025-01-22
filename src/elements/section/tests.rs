use std::sync::Arc;

use crate::elements::section::elem::Section;
use crate::elements::section::rule::section_kind;
use crate::elements::section::style::SectionLinkPos;
use crate::elements::section::style::SectionStyle;
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
# 1
##+ 2
###* 3
####+* 4
#####*+ 5
######{refname} 6
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
		Section { depth == 1, title == "1" };
		Section { depth == 2, title == "2", kind == section_kind::NO_TOC };
		Section { depth == 3, title == "3", kind == section_kind::NO_NUMBER };
		Section { depth == 4, title == "4", kind == section_kind::NO_NUMBER | section_kind::NO_TOC };
		Section { depth == 5, title == "5", kind == section_kind::NO_NUMBER | section_kind::NO_TOC };
		Section { depth == 6, title == "6", reference == Some("refname".to_string()) };
	);
}

#[test]
fn lua() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
%<
nml.section.push("1", 1, "", nil)
nml.section.push("2", 2, "+", nil)
nml.section.push("3", 3, "*", nil)
nml.section.push("4", 4, "+*", nil)
nml.section.push("5", 5, "*+", nil)
nml.section.push("6", 6, "", "refname")
>%
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
		Section { depth == 1, title == "1" };
		Section { depth == 2, title == "2", kind == section_kind::NO_TOC };
		Section { depth == 3, title == "3", kind == section_kind::NO_NUMBER };
		Section { depth == 4, title == "4", kind == section_kind::NO_NUMBER | section_kind::NO_TOC };
		Section { depth == 5, title == "5", kind == section_kind::NO_NUMBER | section_kind::NO_TOC };
		Section { depth == 6, title == "6", reference == Some("refname".to_string()) };
	);
}

#[test]
fn style() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
@@style.section = {
	"link_pos": "None",
	"link": ["a", "b", "c"]
}
		"#
		.to_string(),
		None,
	));
	let parser = LangParser::default();
	let state = ParserState::new(&parser, None);
	let (_, state) = parser.parse(state, source, None, ParseMode::default());

	let style = state
		.shared
		.styles
		.borrow()
		.current(SectionStyle::key())
		.downcast_rc::<SectionStyle>()
		.unwrap();

	assert_eq!(style.link_pos, SectionLinkPos::None);
	assert_eq!(
		style.link,
		["a".to_string(), "b".to_string(), "c".to_string()]
	);
}

#[test]
fn semantics() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
# First section
##{📫}+ test
#{refname}*+ Another section
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
	section_heading { delta_line == 1, delta_start == 0, length == 1 };
	section_name { delta_line == 0, delta_start == 1 };

	section_heading { delta_line == 1, delta_start == 0, length == 2 };
	section_reference { delta_line == 0, delta_start == 2, length == 4 };
	section_kind { delta_line == 0, delta_start == 4, length == 1 };
	section_name { delta_line == 0, delta_start == 1 };

	section_heading { delta_line == 1, delta_start == 0, length == 1 };
	section_reference { delta_line == 0, delta_start == 1, length == 9 };
	section_kind { delta_line == 0, delta_start == 9, length == 2 };
	section_name { delta_line == 0, delta_start == 2 };
	);
}
