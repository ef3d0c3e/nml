use std::sync::Arc;

use crate::elements::graphviz::elem::Graphviz;
use crate::parser::langparser::LangParser;
use crate::parser::parser::ParseMode;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::source::SourceFile;
use crate::validate_document;
use crate::validate_semantics;

#[test]
pub fn parse() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
[graph][width=200px, layout=neato]
Some graph...
[/graph]
[graph]
Another graph
[/graph]
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
		Graphviz { width == "200px", dot == "Some graph..." };
		Graphviz { dot == "Another graph" };
	);
}

#[test]
pub fn lua() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
%<nml.graphviz.push("neato", "200px", "Some graph...")>%
%<nml.graphviz.push("dot", "", "Another graph")>%
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
		Graphviz { width == "200px", dot == "Some graph..." };
		Graphviz { dot == "Another graph" };
	);
}

#[test]
fn semantic() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"
[graph][width=50%]
digraph {
}
[/graph]
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
		graph_sep { delta_line == 1, delta_start == 0, length == 7 };
		graph_props_sep { delta_line == 0, delta_start == 7, length == 1 };
		prop_name { delta_line == 0, delta_start == 1, length == 5 };
		prop_equal { delta_line == 0, delta_start == 5, length == 1 };
		prop_value { delta_line == 0, delta_start == 1, length == 3 };
		graph_props_sep { delta_line == 0, delta_start == 3, length == 1 };
		graph_content { delta_line == 0, delta_start == 1, length == 1 };
		graph_content { delta_line == 1, delta_start == 0, length == 10 };
		graph_content { delta_line == 1, delta_start == 0, length == 2 };
		graph_sep { delta_line == 1, delta_start == 0, length == 8 };
	);
}
