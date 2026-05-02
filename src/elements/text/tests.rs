use std::sync::Arc;

use crate::elements::link::elem::Link;
use crate::elements::meta::eof::Eof;
use crate::elements::meta::scope::ScopeElement;
use crate::elements::text::elem::Text;
use crate::parser::parser::Parser;
use crate::parser::source::SourceFile;
use crate::unit::translation::TranslationUnit;
use crate::validate_ast;

#[test]
fn parser() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"TEXT"#.to_string(),
		None,
	));

	let parser = Parser::new();
	let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
	let (reports, unit) = unit.consume("".into());
	assert!(reports.is_empty());

	validate_ast!(unit.get_entry_scope(), 0,
		Text { content == "TEXT" };
	);
}

#[test]
fn lua() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"TEXT [link](https://url)
{:lua for scope, elem in nml.unit():content(true) do
	nml.unit():add_content(elem)
end:}"#
			.to_string(),
		None,
	));
	let parser = Parser::new();
	let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
	let (reports, unit) = unit.consume("".into());
	assert!(reports.is_empty());

	validate_ast!(unit.get_entry_scope(), 0,
		Text { content == "TEXT " };
		Link;
		ScopeElement [{
			Text { content == "TEXT " };
			Link;
		}];
		Eof;
	);
}
