use std::rc::Rc;

use crate::elements::block::elem::Block;
use crate::elements::block::style::AuthorPos;
use crate::elements::block::style::QuoteStyle;
use crate::elements::paragraph::Paragraph;
use crate::elements::style::Style;
use crate::elements::text::Text;
use crate::parser::langparser::LangParser;
use crate::parser::parser::ParseMode;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::source::SourceFile;
use crate::validate_document;

#[test]
pub fn parser() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
BEFORE
>[!Quote][author=A, cite=B, url=C]
>Some entry
>contin**ued here
>**
AFTER
>    [!Quote]
> Another
>
> quote
>>[!Quote][author=B]
>>Nested
>>> [!Quote]
>>> More nested
AFTER
>[!Warning]
>>[!Note][]
>>>[!Todo]
>>>>[!Tip][]
>>>>>[!Caution]
>>>>>Nested
END
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
	Paragraph { Text{ content == "BEFORE" }; };
	Block {
		Text { content == "Some entry contin" };
		Style;
		Text { content == "ued here" };
		Style;
	};
	Paragraph { Text{ content == "AFTER" }; };
	Block {
		Text { content == "Another" };
		Text { content == " " };
		Text { content == "quote" };
		Block {
			Text { content == "Nested" };
			Block {
				Text { content == "More nested" };
			};
		};
	};
	Paragraph { Text{ content == "AFTER" }; };
	Block {
		Block {
			Block {
				Block {
					Block {
						Text { content == "Nested" };
					};
				};
			};
		};
	};
	Paragraph { Text{ content == "END" }; };
	);
}

#[test]
pub fn style() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
@@style.block.quote = {
	"author_pos": "Before",
	"format": ["{cite} by {author}", "Author: {author}", "From: {cite}"]
}
PRE
>[!Quote][author=A, cite=B, url=C]
>Some entry
>contin**ued here
>**
AFTER
"#
		.to_string(),
		None,
	));
	let parser = LangParser::default();
	let (_, state) = parser.parse(
		ParserState::new(&parser, None),
		source,
		None,
		ParseMode::default(),
	);

	let style = state
		.shared
		.styles
		.borrow()
		.current(QuoteStyle::key())
		.downcast_rc::<QuoteStyle>()
		.unwrap();

	assert_eq!(style.author_pos, AuthorPos::Before);
	assert_eq!(
		style.format,
		[
			"{cite} by {author}".to_string(),
			"Author: {author}".to_string(),
			"From: {cite}".to_string()
		]
	);
}
