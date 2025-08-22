use tower_lsp::lsp_types::CompletionContext;
use tower_lsp::lsp_types::CompletionItem;
use tower_lsp::lsp_types::CompletionItemKind;
use tower_lsp::lsp_types::InsertTextFormat;
use tower_lsp::lsp_types::MarkupContent;

use crate::lsp::completion::context_triggered;
use crate::lsp::completion::CompletionProvider;
use crate::unit::translation::TranslationUnit;

pub struct CodeCompletion;

impl CompletionProvider for CodeCompletion {
	fn trigger(&self) -> &'static [&'static str] {
		["`"].as_slice()
	}

	fn unit_items(&self, _unit: &TranslationUnit, _items: &mut Vec<CompletionItem>) {}

	fn static_items(&self, context: &Option<CompletionContext>, items: &mut Vec<CompletionItem>) {
		items.push(CompletionItem {
			label: "Code Listing".to_string(),
			detail: Some("Code Fragment with Listing".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
				MarkupContent {
					kind: tower_lsp::lsp_types::MarkupKind::Markdown,
					value: "# Usage

```` ```[line_offset=15] Rust, Listing\\n...``` ```` Code listing, with a line offset of 15
``` ```Plain Text, \\n...``` ``` Code listing without a title

The title after the language is optional.
When not using a title, the `,` is optional too.

# Properties

 * `line_offset` Specifies the line numbering offset for the listing (defaults to **0**)
"
					.into(),
				},
			)),
			kind: Some(CompletionItemKind::SNIPPET),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
				"{}``${{1:LANGUAGE}}, ${{2:TITLE}}\n${{3:CONTENT}}\n```",
				if context_triggered(context, "`") {
					""
				} else {
					"`"
				}
			)),
			..CompletionItem::default()
		});

		items.push(CompletionItem {
			label: "Code".to_string(),
			detail: Some("Code Fragment".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
				MarkupContent {
					kind: tower_lsp::lsp_types::MarkupKind::Markdown,
					value: "# Usage

``` ``Plain Text, My Code\\n...`` ``` Code block without line gutter
``` ``Plain Text,\\n...`` ``` Code block without title

The title after the language is optional.
When not using a title, the `,` is optional too.
"
					.into(),
				},
			)),
			kind: Some(CompletionItemKind::SNIPPET),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
				"{}`${{1:LANGUAGE}}, ${{2:TITLE}}\n${{3:CONTENT}}\n``",
				if context_triggered(context, "`") {
					""
				} else {
					"`"
				}
			)),
			..CompletionItem::default()
		});

		items.push(CompletionItem {
			label: "Inline Code".to_string(),
			detail: Some("Inline Code Fragment".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
					MarkupContent {
						kind: tower_lsp::lsp_types::MarkupKind::Markdown,
						value: "# Usage

``` ``C, ...`` ``` Inline code fragment
"
.into(),
					},
			)),
			kind: Some(CompletionItemKind::SNIPPET),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
					"{}`${{1:LANGUAGE}} ${{2:CONTENT}}\n``",
					if context_triggered(context, "`") {
						""
					} else {
						"`"
					}
			)),
			..CompletionItem::default()
		});
	}
}
