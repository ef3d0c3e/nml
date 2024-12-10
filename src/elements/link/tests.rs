use std::rc::Rc;

use crate::elements::link::elem::Link;
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
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
Some [link](url).
[**BOLD link**](another url)
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
			Text { content == "Some " };
			Link { url == "url" } { Text { content == "link" }; };
			Text { content == "." };
			Link { url == "another url" } {
				Style;
				Text { content == "BOLD link" };
				Style;
			};
		};
	);
}

#[test]
fn lua() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
Some %<nml.link.push("link", "url")>%.
%<
nml.link.push("**BOLD link**", "another url")
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
		Paragraph {
			Text { content == "Some " };
			Link { url == "url" } { Text { content == "link" }; };
			Text { content == "." };
			Link { url == "another url" } {
				Style;
				Text { content == "BOLD link" };
				Style;
			};
		};
	);
}

#[test]
fn semantics() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
 -  [la\](*testi*nk](url)
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
	list_bullet { delta_line == 1, delta_start == 1, length == 1 };
	link_display_sep { delta_line == 0, delta_start == 3, length == 1 };
	style_marker { delta_line == 0, delta_start == 6, length == 1 };
	style_marker { delta_line == 0, delta_start == 6, length == 1 };
	link_display_sep { delta_line == 0, delta_start == 3, length == 1 };
	link_url_sep { delta_line == 0, delta_start == 1, length == 1 };
	link_url { delta_line == 0, delta_start == 1, length == 3 };
	link_url_sep { delta_line == 0, delta_start == 3, length == 1 };
	);
}
