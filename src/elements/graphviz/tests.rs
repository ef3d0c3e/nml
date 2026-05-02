use std::sync::Arc;

use graphviz_rust::cmd::Layout;

use crate::elements::graphviz::elem::Graphviz;
use crate::elements::linebreak::elem::LineBreak;
use crate::elements::meta::eof::Eof;
use crate::elements::meta::scope::ScopeElement;
use crate::elements::raw::elem::Raw;
use crate::layout::size::Size;
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
[graph]
test1
[/graph]
[graph][width=1000px, layout=sfdp]
test2
[/graph]
"#
		.to_string(),
		None,
	));

	let parser = Parser::new();
	let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
	let (reports, unit) = unit.consume("".into());
	assert!(reports.is_empty());

	validate_ast!(unit.get_entry_scope(), 0,
		Graphviz {
			graph == "test1",
		};
		Graphviz {
			graph == "test2",
			width == Size::Px(1000f64),
			//layout == Layout::Sfdp https://github.com/besok/graphviz-rust/pull/50
		};
		LineBreak;
		Eof;
	);
}

#[test]
fn lua() {
	let source = Arc::new(SourceFile::with_content(
		"".to_string(),
		r#"{:lua
nml.unit():add_content(nml.graphviz.Graphviz("dot", nil, "test1"))
nml.unit():add_content(nml.graphviz.Graphviz("sfdp", "1000px", "test2"))
:}
"#
		.to_string(),
		None,
	));

	let parser = Parser::new();
	let unit = TranslationUnit::new("".into(), Arc::new(parser), source, false, false);
	let (reports, unit) = unit.consume("".into());
	assert!(reports.is_empty());

	validate_ast!(unit.get_entry_scope(), 0,
	ScopeElement [
		{
			Graphviz {
				graph == "test1",
			};
			Graphviz {
				graph == "test2",
				width == Size::Px(1000f64),
				//layout == Layout::Sfdp https://github.com/besok/graphviz-rust/pull/50
			};
		}
	];
	LineBreak;
	Eof;
	);
}
