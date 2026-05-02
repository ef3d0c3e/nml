use std::sync::Arc;

use crate::elements::comment::elem::Comment;
use crate::elements::linebreak::elem::LineBreak;
use crate::elements::meta::eof::Eof;
use crate::elements::raw::elem::Raw;
use crate::elements::text::elem::Text;
use crate::parser::parser::Parser;
use crate::parser::source::SourceFile;
use crate::unit::element::ElemKind;
use crate::unit::translation::TranslationUnit;
use crate::validate_ast;

#[test]
fn parser() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
::test
foo::bar
foo ::bar
"#.to_string(),
		None,
	));

	let parser = Parser::new();
	let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
	let (reports, unit) = unit.consume("".into());
	assert!(reports.is_empty());

	validate_ast!(unit.get_entry_scope(), 0,
		Comment { content == "test" };
		Text { content == "foo::bar foo" };
		Comment { content == "bar" };
		LineBreak;
		Eof;
	);
}
