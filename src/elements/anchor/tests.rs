use std::sync::Arc;

use crate::elements::anchor::elem::Anchor;
use crate::elements::code::elem::Code;
use crate::elements::heading::elem::Heading;
use crate::elements::linebreak::elem::LineBreak;
use crate::elements::meta::eof::Eof;
use crate::elements::meta::scope::ScopeElement;
use crate::elements::text::elem::Text;
use crate::parser::parser::Parser;
use crate::parser::source::SourceFile;
use crate::unit::references::Refname;
use crate::unit::translation::TranslationUnit;
use crate::validate_ast;

#[test]
fn parser() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#":anchor an1:
Inside :anchor an2: paragraph.

# :anchor an3: Heading
"#
		.to_string(),
		None,
	));

	let parser = Parser::new();
	let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
	let (reports, unit) = unit.consume("".into());
	assert!(reports.is_empty());

	validate_ast!(unit.get_entry_scope(), 0,
		Anchor { refname == Refname::Internal("an1".into()) };
		Text { content == "Inside ".to_string() };
		Anchor { refname == Refname::Internal("an2".into()) };
		Text { content == " paragraph.".to_string() };
		LineBreak;
		Heading [
			{
				Anchor { refname == Refname::Internal("an3".into()) };
				Text { content == " Heading".to_string() };
				Eof;
			}
		];
		LineBreak;
		Eof;
	);
}

#[test]
fn lua() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"{:lua  nml.unit():add_content(nml.anchor.Anchor("an1")):}
"#
		.to_string(),
		None,
	));

	let parser = Parser::new();
	let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
	let (reports, unit) = unit.consume("".into());
	println!("{reports:#?}");
	assert!(reports.is_empty());

	validate_ast!(unit.get_entry_scope(), 0,
		ScopeElement [
			{ Anchor { refname == Refname::Internal("an1".into()) }; }
		];
		LineBreak;
		Eof;
	);
}
