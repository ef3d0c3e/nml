use std::rc::Rc;

use crate::compiler::compiler::Target;
use crate::compiler::process::process_from_memory;
use crate::document::document::CrossReference;
use crate::elements::paragraph::elem::Paragraph;
use crate::elements::reference::elem::ExternalReference;
use crate::elements::reference::elem::InternalReference;
use crate::elements::section::elem::Section;
use crate::parser::langparser::LangParser;
use crate::parser::parser::ParseMode;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::source::SourceFile;
use crate::validate_document;

#[test]
pub fn parse_internal() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
#{ref} Referenceable section

&{ref}[caption=Section]
&{ref}[caption=Another]
&{ref2}[caption=Before]

#{ref2} Another section
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
		Section;
		Paragraph {
			InternalReference { refname == "ref", caption == Some("Section".to_string()) };
			InternalReference { refname == "ref", caption == Some("Another".to_string()) };
			InternalReference { refname == "ref2", caption == Some("Before".to_string()) };
		};
		Paragraph;
		Section;
	);
}

#[test]
pub fn parse_external() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
&{DocA#ref}[caption=Section]
&{DocB#ref}
&{#ref}[caption='ref' from any document]
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
			ExternalReference { reference == CrossReference::Specific("DocA".into(), "ref".into()), caption == Some("Section".to_string()) };
			ExternalReference { reference == CrossReference::Specific("DocB".into(), "ref".into()), caption == None::<String> };
			ExternalReference { reference == CrossReference::Unspecific("ref".into()), caption == Some("'ref' from any document".to_string()) };
		};
	);
}

#[test]
pub fn test_external() {
	let result = process_from_memory(
		Target::HTML,
		vec![
			r#"
@html.page_title = 0
@compiler.output = a.html

#{ref} Referenceable section
"#
			.into(),
			r#"
@html.page_title = 1
@compiler.output = b.html

&{#ref}
&{a#ref}
#{ref2} Another Referenceable section
"#
			.into(),
			r#"
@html.page_title = 2

@@style.external_reference = {
	"format_unspecific": "[UNSPECIFIC {refname}]",
	"format_specific": "[SPECIFIC {refdoc}:{refname}]"
}

&{#ref}[caption=from 0]
&{#ref}
&{#ref2}[caption=from 1]
&{b#ref2}
"#
			.into(),
		],
	)
	.unwrap();

	assert!(result[1].0.borrow().body.starts_with("<div class=\"content\"><p><a href=\"a.html#Referenceable_section\">(#ref)</a><a href=\"a.html#Referenceable_section\">(a#ref)</a></p>"));
	assert!(result[2].0.borrow().body.starts_with("<div class=\"content\"><p><a href=\"a.html#Referenceable_section\">from 0</a><a href=\"a.html#Referenceable_section\">[UNSPECIFIC ref]</a><a href=\"b.html#Another_Referenceable_section\">from 1</a><a href=\"b.html#Another_Referenceable_section\">[SPECIFIC b:ref2]</a></p>"));
}
