use std::sync::Arc;

use crate::elements::paragraph::elem::Paragraph;
use crate::elements::text::elem::Text;
use crate::parser::parser::Parser;
use crate::parser::source::SourceFile;

#[test]
fn parse() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
First paragraph
Second line

Second paragraph\
<- literal \\n


Last paragraph
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
		Text { content == "First paragraph Second line" };
	};
	Paragraph {
		Text { content == "Second paragraph\n<- literal \\n" };
	};
	Paragraph {
		Text { content == "Last paragraph " };
	};
	);
}
