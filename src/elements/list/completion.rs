use tower_lsp::lsp_types::CompletionContext;
use tower_lsp::lsp_types::CompletionItem;
use tower_lsp::lsp_types::CompletionItemKind;
use tower_lsp::lsp_types::InsertTextFormat;
use tower_lsp::lsp_types::MarkupContent;

use crate::lsp::completion::CompletionProvider;
use crate::unit::translation::TranslationUnit;

pub struct ListCompletion;

impl CompletionProvider for ListCompletion {
	fn trigger(&self) -> &'static [&'static str] {
		["*", "-"].as_slice()
	}

	fn unit_items(&self, _unit: &TranslationUnit, _items: &mut Vec<CompletionItem>) {}

	fn static_items(&self, _context: &Option<CompletionContext>, items: &mut Vec<CompletionItem>) {
		// *
		items.push(CompletionItem {
			label: "*".to_string(),
			detail: Some("Unnumbered List".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
				MarkupContent {
					kind: tower_lsp::lsp_types::MarkupKind::Markdown,
					value: "# Usage

List block: ```
┃ * First entry
┃ * Second entry
┃ *- Nested entry```

TODO List: ```
┃ * [X] Done
┃ * [-] In progress
┃ * [ ] TODO```

# Properties

 * `offset` Numbering offset (**defaults to `1`**)

# See also

 * `-` *numbered list*

"
					.into(),
				},
			)),
			kind: Some(CompletionItemKind::SNIPPET),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
				"* ${{1:CONTENT}}"
			)),
			..CompletionItem::default()
		});

		// -
		items.push(CompletionItem {
			label: "-".to_string(),
			detail: Some("Numbered List".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
				MarkupContent {
					kind: tower_lsp::lsp_types::MarkupKind::Markdown,
					value: "# Usage

List block: ```
┃ - First entry
┃ - Second entry
┃ -* Nested entry
┃ -[offset=32] Last entry```

TODO List: ```
┃ - [X] Done
┃ - [-] In progress
┃ - [ ] TODO```

# Properties

 * `offset` Numbering offset (**defaults to `1`**)

# See also

 * `*` *unnumbered list*

"
					.into(),
				},
			)),
			kind: Some(CompletionItemKind::SNIPPET),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
				"- ${{1:CONTENT}}"
			)),
			..CompletionItem::default()
		});
	}
}
