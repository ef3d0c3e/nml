use crate::elements::comment::elem::Comment;
use crate::elements::paragraph::elem::Paragraph;
use crate::elements::style::elem::Style;
use crate::elements::text::Text;
use crate::parser::langparser::LangParser;
use crate::parser::parser::ParseMode;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::source::SourceFile;
use crate::validate_document;
use crate::validate_semantics;
use std::rc::Rc;

#[test]
fn parser() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
NOT COMMENT: `std::cmp`
:: Commented line
COMMENT ::Test
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
		Text; Style; Text; Style;
		Comment { content == "Commented line" };
		Text; Comment { content == "Test" };
	};
	);
}

#[test]
fn semantic() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
::Test
 ::Another
	:: Another
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
	comment { delta_line == 1, delta_start == 0, length == 6 };
	comment { delta_line == 1, delta_start == 1, length == 9 };
	comment { delta_line == 1, delta_start == 1, length == 10 };
	);
}
