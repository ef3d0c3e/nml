use std::rc::Rc;

use crate::parser::langparser::LangParser;
use crate::parser::parser::ParseMode;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::source::SourceFile;
use crate::validate_semantics;

#[test]
fn semantics() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
@@style.section = {
	"link_pos": "Before",
	"link": ["", "⛓️", "       "]
}
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
		elemstyle_operator { delta_line == 1, delta_start == 0, length == 2 };
		elemstyle_name { delta_line == 0, delta_start == 2, length == 14 };
		elemstyle_equal { delta_line == 0, delta_start == 14, length == 1 };
		elemstyle_value { delta_line == 0, delta_start == 2, length == 2 };
		elemstyle_value { delta_line == 1, delta_start == 0, length == 23 };
		elemstyle_value { delta_line == 1, delta_start == 0, length == 31 };
		elemstyle_value { delta_line == 1, delta_start == 0, length == 2 };
	);
}
