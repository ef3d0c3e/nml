use std::sync::Arc;

use crate::elements::paragraph::elem::Paragraph;
use crate::elements::tex::elem::Tex;
use crate::parser::langparser::LangParser;
use crate::parser::parser::ParseMode;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::source::SourceFile;
use crate::validate_document;
use crate::validate_semantics;

#[test]
fn tex_block() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
$[kind=block, caption=Some\, text\\] 1+1=2	$
$|[env=another] Non Math \LaTeX |$
$[kind=block,env=another] e^{i\pi}=-1$
%<nml.tex.push_math("block", "1+1=2", nil, "Some, text\\")>%
%<nml.tex.push("block", "Non Math \\LaTeX", "another", nil)>%
%<nml.tex.push_math("block", "e^{i\\pi}=-1", "another", nil)>%
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
		Tex { mathmode == true, tex == "1+1=2", env == "main", caption == Some("Some, text\\\\".to_string()) };
		Tex { mathmode == false, tex == "Non Math \\LaTeX", env == "another" };
		Tex { mathmode == true, tex == "e^{i\\pi}=-1", env == "another" };
		Tex { mathmode == true, tex == "1+1=2", env == "main", caption == Some("Some, text\\".to_string()) };
		Tex { mathmode == false, tex == "Non Math \\LaTeX", env == "another" };
		Tex { mathmode == true, tex == "e^{i\\pi}=-1", env == "another" };
	);
}

#[test]
fn tex_inline() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
$[ caption=Some\, text\\] 1+1=2	$
$|[env=another, kind=inline  ,   caption = Enclosed \].  ] Non Math \LaTeX|$
$[env=another] e^{i\pi}=-1$
%<nml.tex.push_math("inline", "1+1=2", "main", "Some, text\\")>%
%<nml.tex.push("inline", "Non Math \\LaTeX", "another", "Enclosed ].")>%
%<nml.tex.push_math("inline", "e^{i\\pi}=-1", "another", nil)>%
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
			Tex { mathmode == true, tex == "1+1=2", env == "main", caption == Some("Some, text\\\\".to_string()) };
			Tex { mathmode == false, tex == "Non Math \\LaTeX", env == "another", caption == Some("Enclosed ].".to_string()) };
			Tex { mathmode == true, tex == "e^{i\\pi}=-1", env == "another" };
			Tex { mathmode == true, tex == "1+1=2", env == "main", caption == Some("Some, text\\".to_string()) };
			Tex { mathmode == false, tex == "Non Math \\LaTeX", env == "another", caption == Some("Enclosed ].".to_string()) };
			Tex { mathmode == true, tex == "e^{i\\pi}=-1", env == "another" };
		};
	);
}

#[test]
fn semantic() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
$[kind=inline]\LaTeX$
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
		tex_sep { delta_line == 1, delta_start == 0, length == 1 };
		tex_props_sep { delta_line == 0, delta_start == 1, length == 1 };
		prop_name { delta_line == 0, delta_start == 1, length == 4 };
		prop_equal { delta_line == 0, delta_start == 4, length == 1 };
		prop_value { delta_line == 0, delta_start == 1, length == 6 };
		tex_props_sep { delta_line == 0, delta_start == 6, length == 1 };
		tex_content { delta_line == 0, delta_start == 1, length == 6 };
		tex_sep { delta_line == 0, delta_start == 6, length == 1 };
	);
}
