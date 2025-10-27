use std::sync::Arc;

use crate::elements::list::elem::List;
use crate::elements::text::elem::Text;
use crate::parser::parser::Parser;
use crate::parser::source::SourceFile;
use crate::unit::translation::TranslationUnit;
use crate::validate_ast;

#[test]
fn parser() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
 * first
 * second
	multi
 line
 - third

 * new list
"#
		.to_string(),
		None,
	));

	let parser = Parser::new();
	let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
	let (reports, unit) = unit.consume("".into());
	eprintln!("{:#?}", unit.get_entry_scope());
	assert!(reports.is_empty());

	validate_ast!(unit.get_entry_scope(), 0,
		List [
			{ Text { content == "first" }; }
			{ Text { content == "second \tmulti  line" }; }
			{ Text { content == "third" }; }
		];
		List [
			{ Text { content == "new list" }; }
		];
	);
}
