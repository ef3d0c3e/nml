use tower_lsp::lsp_types::CompletionContext;
use tower_lsp::lsp_types::CompletionItem;
use tower_lsp::lsp_types::CompletionItemKind;
use tower_lsp::lsp_types::InsertTextFormat;
use tower_lsp::lsp_types::MarkupContent;

use crate::lsp::completion::context_triggered;
use crate::lsp::completion::CompletionProvider;
use crate::unit::translation::TranslationUnit;

pub struct TaggedCompletion;

impl CompletionProvider for TaggedCompletion {
	fn trigger(&self) -> &'static [&'static str] {
		[":"].as_slice()
	}

	fn unit_items(&self, _unit: &TranslationUnit, _items: &mut Vec<CompletionItem>) {}

	fn static_items(&self, context: &Option<CompletionContext>, items: &mut Vec<CompletionItem>) {
		// @import
		items.push(CompletionItem {
			label: ":tagged ".to_string(),
			detail: Some("Set a processor for tagged content".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
				MarkupContent {
					kind: tower_lsp::lsp_types::MarkupKind::Markdown,
					value: "# Usage

`:tagged NAME MODE/PROCESSOR`

Set the tagged processor for NAME, in mode MODE using processor PROCESSOR.

NAME is the name of the tag, for instance `{@note ...}` would have name `note`.
MODE is the parsing mode for the tagged content, must be `raw` or `parsed`
 - `raw` Keeps the content as raw text and passes buffer ranges to Lua
 - `parsed` Parses the content and passes the parsed scopes to Lua
PROCESSOR is the name of the Lua function that will handle the tagged content

# Example

 * `:tagged note parsed/note_processor` Parse tagged @note, and pass the resulting scopes to function `note_processor`
"
						.into(),
				},
			)),
			kind: Some(CompletionItemKind::FUNCTION),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
				"{}tagged ${{1:NAME}} ${{2:MODE}}/${{3:PROCESSOR}}",
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
