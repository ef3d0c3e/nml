use std::sync::Arc;

use crate::elements::table::elem::Align;
use crate::parser::langparser::LangParser;
use crate::parser::parser::ParseMode;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::source::SourceFile;

use super::elem::Table;

#[test]
pub fn parser() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
| :rvspan=3: 0 | :talign=right: 1 | :chspan=2: |
| :hspan=2: 2  | :align=center: test           |

:TABLE {refname} Title
| A | B |
| 1 | 2 |
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

	let borrow = doc.content().borrow_mut();
	let table1 = &borrow[0].downcast_ref::<Table>().unwrap();
	assert_eq!(table1.size, (3, 2));
	assert_eq!(table1.properties.align, Some(Align::Right));
	assert_eq!(table1.rows[0].as_ref().and_then(|row| row.vspan), Some(3));
	assert_eq!(
		table1.columns[2].as_ref().and_then(|col| col.hspan),
		Some(2)
	);

	let table2 = &borrow[2].downcast_ref::<Table>().unwrap();
	assert_eq!(table2.size, (2, 2));
	assert_eq!(table2.reference, Some("refname".to_string()));
	assert_eq!(table2.title, Some("Title".to_string()));
}
