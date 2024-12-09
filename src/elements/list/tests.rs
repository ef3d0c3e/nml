use std::rc::Rc;

use crate::elements::list::elem::CheckboxState;
use crate::elements::list::elem::CustomListData;
use crate::elements::list::elem::ListEntry;
use crate::elements::list::elem::ListMarker;
use crate::elements::list::elem::MarkerKind;
use crate::elements::paragraph::Paragraph;
use crate::elements::text::Text;
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
 * 1
 *[offset=7] 2
	continued
 * 3

 * New list
 *-[bullet=(*)] A
 *- B
 * Back
 *-* More nested


 * [X] Checked
 * [x] Checked
 * [-] Partial
 * [] Unchecked
 * [ ] Unchecked
"#
		.to_string(),
		None,
	));
	let parser = LangParser::default();
	let state = ParserState::new(&parser, None);
	let (doc, _) = parser.parse(state, source, None, ParseMode::default());

	validate_document!(doc.content().borrow(), 0,
		ListMarker { numbered == false, kind == MarkerKind::Open };
		ListEntry { numbering == vec![(false, 1)] } {
			Text { content == "1" };
		};
		ListEntry { numbering == vec![(false, 7)] } {
			Text { content == "2 continued" };
		};
		ListEntry { numbering == vec![(false, 8)] } {
			Text { content == "3" };
		};
		ListMarker { numbered == false, kind == MarkerKind::Close };

		Paragraph;

		ListMarker { numbered == false, kind == MarkerKind::Open };
		ListEntry { numbering == vec![(false, 1)] } {
			Text { content == "New list" };
		};
		ListMarker { numbered == true, kind == MarkerKind::Open };
			ListEntry { numbering == vec![(false, 2), (true, 1)], bullet == Some("(*)".to_string()) } {
				Text { content == "A" };
			};
			ListEntry { numbering == vec![(false, 2), (true, 2)], bullet == Some("(*)".to_string()) } {
				Text { content == "B" };
			};
		ListMarker { numbered == true, kind == MarkerKind::Close };
		ListEntry { numbering == vec![(false, 2)] } {
			Text { content == "Back" };
		};
		ListMarker { numbered == true, kind == MarkerKind::Open };
		ListMarker { numbered == false, kind == MarkerKind::Open };
		ListEntry { numbering == vec![(false, 3), (true, 1), (false, 1)] } {
			Text { content == "More nested" };
		};
		ListMarker { numbered == false, kind == MarkerKind::Close };
		ListMarker { numbered == true, kind == MarkerKind::Close };
		ListMarker { numbered == false, kind == MarkerKind::Close };
		Paragraph;
		ListMarker { numbered == false, kind == MarkerKind::Open };
		ListEntry { custom == Some(CustomListData::Checkbox(CheckboxState::Checked)) };
		ListEntry { custom == Some(CustomListData::Checkbox(CheckboxState::Checked)) };
		ListEntry { custom == Some(CustomListData::Checkbox(CheckboxState::Partial)) };
		ListEntry { custom == Some(CustomListData::Checkbox(CheckboxState::Unchecked)) };
		ListEntry { custom == Some(CustomListData::Checkbox(CheckboxState::Unchecked)) };
		ListMarker { numbered == false, kind == MarkerKind::Close };
	);
}

#[test]
fn semantic() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
 *[offset=5] First **bold**
	Second line
 *- Another
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
		list_props_sep { delta_line == 0, delta_start == 1, length == 1 };
		prop_name { delta_line == 0, delta_start == 1, length == 6 };
		prop_equal { delta_line == 0, delta_start == 6, length == 1 };
		prop_value { delta_line == 0, delta_start == 1, length == 1 };
		list_props_sep { delta_line == 0, delta_start == 1, length == 1 };
		style_marker { delta_line == 0, delta_start == 8, length == 2 };
		style_marker { delta_line == 0, delta_start == 6, length == 2 };
		list_bullet { delta_line == 2, delta_start == 1, length == 2 };
	);
}
