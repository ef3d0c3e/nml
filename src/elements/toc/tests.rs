use std::rc::Rc;

use crate::elements::section::elem::Section;
use crate::elements::toc::elem::Toc;
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
#+TABLE_OF_CONTENT TOC
# Section1
## SubSection
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
		Toc { title == Some("TOC".to_string()) };
		Section;
		Section;
	);
}

#[test]
fn lua() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
%<nml.toc.push("TOC")>%
%<nml.toc.push()>%
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
		Toc { title == Some("TOC".to_string()) };
		Toc { title == Option::<String>::None };
	);
}

#[test]
fn semantic() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
#+TABLE_OF_CONTENT TOC
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
		toc_sep { delta_line == 1, delta_start == 0, length == 2 };
		toc_token { delta_line == 0, delta_start == 2, length == 16 };
		toc_title { delta_line == 0, delta_start == 16, length == 4 };
	);
}
