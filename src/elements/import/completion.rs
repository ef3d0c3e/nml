use tower_lsp::lsp_types::CompletionContext;
use tower_lsp::lsp_types::CompletionItem;
use tower_lsp::lsp_types::CompletionItemKind;
use tower_lsp::lsp_types::InsertTextFormat;
use tower_lsp::lsp_types::MarkupContent;

use crate::lsp::completion::context_triggered;
use crate::lsp::completion::CompletionProvider;
use crate::unit::translation::TranslationUnit;

pub struct ImportCompletion;

impl CompletionProvider for ImportCompletion {
	fn trigger(&self) -> &'static [&'static str] {
		["@"].as_slice()
	}

	fn unit_items(&self, _unit: &TranslationUnit, _items: &mut Vec<CompletionItem>) {}

	fn static_items(&self, context: &Option<CompletionContext>, items: &mut Vec<CompletionItem>) {
		// @import
		items.push(CompletionItem {
			label: "@import ".to_string(),
			detail: Some("Import a file".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
				MarkupContent {
					kind: tower_lsp::lsp_types::MarkupKind::Markdown,
					value: "# Usage

`@import FILE`

Import another file's content. The content of the imported file is added to the
current file at the location of the import call.

When importing a file, all references will be exported to the importing file.
Variables set using `:set` will not be exported. If you want variables to be
available in subsequent imports, you need to use `:export` when defining variables.

# Examples

 * `@import \"source.nml\"` *imports file `source.nml` into the current file*

[current.nml]():
┃Content
┃
┃@import \"footer.nml\"

[footer.nml]():
┃Footer

Will result in:
[current.nml]():
┃Content
┃
┃Footer

# See also

 * `:export` *export a variable*"
						.into(),
				},
			)),
			kind: Some(CompletionItemKind::FUNCTION),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
				"{}import \"${{1:FILE}}\"",
				if context_triggered(context, "@") {
					""
				} else {
					"@"
				}
			)),
			..CompletionItem::default()
		});
	}
}
