use std::sync::Arc;

use crate::elements::raw::elem::Raw;
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
{<inline foo
bar>}
{<block baz>}
{<invisible quz>}
"#.to_string(),
		None,
	));

	let parser = Parser::new();
	let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
	let (reports, unit) = unit.consume("".into());
	assert!(reports.is_empty());

	validate_ast!(unit.get_entry_scope(), 0,
		Raw {
			kind == ElemKind::Inline,
			content == "foo\nbar"
		};
		Raw {
			kind == ElemKind::Block,
			content == "baz"
		};
		Raw {
			kind == ElemKind::Invisible,
			content == "quz"
		};
	);
}
