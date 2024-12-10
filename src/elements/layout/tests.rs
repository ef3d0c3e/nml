use std::rc::Rc;

use crate::elements::layout::custom::LayoutToken;
use crate::elements::layout::elem::Layout;
use crate::elements::paragraph::elem::Paragraph;
use crate::elements::text::Text;
use crate::parser::langparser::LangParser;
use crate::parser::parser::ParseMode;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::source::Source;
use crate::parser::source::SourceFile;
use crate::validate_document;
use crate::validate_semantics;

#[test]
fn parser() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
#+LAYOUT_BEGIN[style=A] Split
	A
	#+LAYOUT_BEGIN[style=B] Centered
		B
	#+LAYOUT_END
#+LAYOUT_NEXT[style=C]
	C
	#+LAYOUT_BEGIN[style=D] Split
		D
	#+LAYOUT_NEXT[style=E]
		E
	#+LAYOUT_END
#+LAYOUT_END
#+LAYOUT_BEGIN[title=F] Spoiler
	F
#+LAYOUT_END
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
		Layout { token == LayoutToken::Begin, id == 0 };
		Paragraph {
			Text { content == "A" };
		};
		Layout { token == LayoutToken::Begin, id == 0 };
		Paragraph {
			Text { content == "B" };
		};
		Layout { token == LayoutToken::End, id == 1 };
		Layout { token == LayoutToken::Next, id == 1 };
		Paragraph {
			Text { content == "C" };
		};
		Layout { token == LayoutToken::Begin, id == 0 };
		Paragraph {
			Text { content == "D" };
		};
		Layout { token == LayoutToken::Next, id == 1 };
		Paragraph {
			Text { content == "E" };
		};
		Layout { token == LayoutToken::End, id == 2 };
		Layout { token == LayoutToken::End, id == 2 };

		Layout { token == LayoutToken::Begin, id == 0 };
		Paragraph {
			Text { content == "F" };
		};
		Layout { token == LayoutToken::End, id == 1 };
	);
}

#[test]
fn lua() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
%<nml.layout.push("begin", "Split", "style=A")>%
	A
%<nml.layout.push("Begin", "Centered", "style=B")>%
		B
%<nml.layout.push("end", "Centered", "")>%
%<nml.layout.push("next", "Split", "style=C")>%
	C
%<nml.layout.push("Begin", "Split", "style=D")>%
		D
%<nml.layout.push("Next", "Split", "style=E")>%
		E
%<nml.layout.push("End", "Split", "")>%
%<nml.layout.push("End", "Split", "")>%

%<nml.layout.push("Begin", "Spoiler", "title=Test Spoiler")>%
	F
%<nml.layout.push("End", "Spoiler", "")>%
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
		Layout { token == LayoutToken::Begin, id == 0 };
		Paragraph {
			Text { content == "A" };
		};
		Layout { token == LayoutToken::Begin, id == 0 };
		Paragraph {
			Text { content == "B" };
		};
		Layout { token == LayoutToken::End, id == 1 };
		Layout { token == LayoutToken::Next, id == 1 };
		Paragraph {
			Text { content == "C" };
		};
		Layout { token == LayoutToken::Begin, id == 0 };
		Paragraph {
			Text { content == "D" };
		};
		Layout { token == LayoutToken::Next, id == 1 };
		Paragraph {
			Text { content == "E" };
		};
		Layout { token == LayoutToken::End, id == 2 };
		Layout { token == LayoutToken::End, id == 2 };
		Paragraph;
		Layout { token == LayoutToken::Begin, id == 0 };
		Paragraph {
			Text { content == "F" };
		};
		Layout { token == LayoutToken::End, id == 1 };
	);
}

#[test]
fn semantic() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
#+LAYOUT_BEGIN Split
	#+LAYOUT_NEXT[style=aa]
#+LAYOUT_END
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
		layout_sep { delta_line == 1, delta_start == 0, length == 2 };
		layout_token { delta_line == 0, delta_start == 2, length == 12 };
		layout_type { delta_line == 0, delta_start == 12, length == 6 };
		layout_sep { delta_line == 1, delta_start == 1, length == 2 };
		layout_token { delta_line == 0, delta_start == 2, length == 11 };
		layout_props_sep { delta_line == 0, delta_start == 11, length == 1 };
		prop_name { delta_line == 0, delta_start == 1, length == 5 };
		prop_equal { delta_line == 0, delta_start == 5, length == 1 };
		prop_value { delta_line == 0, delta_start == 1, length == 2 };
		layout_props_sep { delta_line == 0, delta_start == 2, length == 1 };
		layout_sep { delta_line == 1, delta_start == 0, length == 2 };
		layout_token { delta_line == 0, delta_start == 2, length == 10 };
	);
}

#[test]
fn hints() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
#+LAYOUT_BEGIN Split
	A
#+LAYOUT_NEXT
	B
#+LAYOUT_END
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
	if let Some(lsp) = &state.shared.lsp {
		let borrow = lsp.borrow();

		if let Some(hints) = borrow.inlay_hints.get(&(source as Rc<dyn Source>)) {
			let borrow = hints.hints.borrow();
			assert_eq!(
				borrow[0].position,
				tower_lsp::lsp_types::Position {
					line: 3,
					character: 13
				}
			);
			assert_eq!(
				borrow[1].position,
				tower_lsp::lsp_types::Position {
					line: 5,
					character: 12
				}
			);
		}
	}
}
