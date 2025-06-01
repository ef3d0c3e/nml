use tower_lsp::lsp_types::CompletionContext;
use tower_lsp::lsp_types::CompletionItem;
use tower_lsp::lsp_types::CompletionItemKind;
use tower_lsp::lsp_types::InsertTextFormat;
use tower_lsp::lsp_types::MarkupContent;

use crate::lsp::completion::context_triggered;
use crate::lsp::completion::CompletionProvider;
use crate::unit::translation::TranslationUnit;

pub struct AnchorCompletion;

impl CompletionProvider for AnchorCompletion {
	fn trigger(&self) -> &'static [&'static str] {
		[":"].as_slice()
	}

	fn unit_items(&self, _unit: &TranslationUnit, _items: &mut Vec<CompletionItem>) {}

	fn static_items(&self, context: &Option<CompletionContext>, items: &mut Vec<CompletionItem>) {
		// @import
		items.push(CompletionItem {
			label: ":anchor ".to_string(),
			detail: Some("Creates an anchor".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
				MarkupContent {
					kind: tower_lsp::lsp_types::MarkupKind::Markdown,
					value: "# Usage

`:anchor NAME:`

Creates an anchor at the location of the `:anchor` command.
Created anchors can then be linked against using internal links.

# Examples

 * `:anchor foo:` *creates an anchor named **foo** here*

┃:anchor bar:
┃
┃Link to anchor: &{bar}"
						.into(),
				},
			)),
			kind: Some(CompletionItemKind::FUNCTION),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
				"{}anchor ${{1:NAME}}:",
				if context_triggered(context, ":") {
					""
				} else {
					":"
				}
			)),
			..CompletionItem::default()
		});
	}
}
