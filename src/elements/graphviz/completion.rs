use tower_lsp::lsp_types::CompletionContext;
use tower_lsp::lsp_types::CompletionItem;
use tower_lsp::lsp_types::CompletionItemKind;
use tower_lsp::lsp_types::InsertTextFormat;
use tower_lsp::lsp_types::MarkupContent;

use crate::lsp::completion::context_triggered;
use crate::lsp::completion::CompletionProvider;
use crate::unit::translation::TranslationUnit;

pub struct GraphvizCompletion;

impl CompletionProvider for GraphvizCompletion {
	fn trigger(&self) -> &'static [&'static str] {
		["["].as_slice()
	}

	fn unit_items(&self, _unit: &TranslationUnit, _items: &mut Vec<CompletionItem>) {}

	fn static_items(&self, context: &Option<CompletionContext>, items: &mut Vec<CompletionItem>) {
		items.push(CompletionItem {
			label: "[graph]".to_string(),
			detail: Some("Graphiv Graph".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
				MarkupContent {
					kind: tower_lsp::lsp_types::MarkupKind::Markdown,
					value: "# Usage

`[graph] ... [/graph]` *render Graphviz graph*
`[graph][width=50%] ... [/graph]` *render Graphviz graph and displys as 50% page width*

# Properties

 * `layout` Specifies the layout engine for Graphviz (defaults to **dot**)
		- **dot** hierarchical or layered drawings of directed graphs.
		- **neato** spring model layouts.
		- **fdp** stands for Force-Directed Placement.
		- **sfdp** stands for Scalable Force-Directed Placement.
		- **circo** circular layout.
		- **twopi** radial layout.
		- **nop** Pretty-print DOT graph file. Equivalent to nop1.
		- **nop2** Pretty-print DOT graph file, assuming positions already known.
		- **osage** draws clustered graphs.
		- **patchwork** draws map of clustered graph using a squarified treemap layout.
 * `width` Graph display width (defaults to **100%**)
"
					.into(),
				},
			)),
			kind: Some(CompletionItemKind::SNIPPET),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
				"{}graph]\n${{1:GRAPH}}\n[/graph]",
				if context_triggered(context, "[") {
					""
				} else {
					"["
				}
			)),
			..CompletionItem::default()
		});
	}
}
