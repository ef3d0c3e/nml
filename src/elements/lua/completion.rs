use tower_lsp::lsp_types::CompletionContext;
use tower_lsp::lsp_types::CompletionItem;
use tower_lsp::lsp_types::CompletionItemKind;
use tower_lsp::lsp_types::InsertTextFormat;
use tower_lsp::lsp_types::MarkupContent;

use crate::lsp::completion::context_triggered;
use crate::lsp::completion::CompletionProvider;
use crate::unit::translation::TranslationUnit;

pub struct LuaCompletion;

impl CompletionProvider for LuaCompletion {
	fn trigger(&self) -> &'static [&'static str] {
		[":"].as_slice()
	}

	fn unit_items(&self, _unit: &TranslationUnit, _items: &mut Vec<CompletionItem>) {}

	fn static_items(&self, context: &Option<CompletionContext>, items: &mut Vec<CompletionItem>) {
		// :lua
		items.push(CompletionItem {
			label: ":lua".to_string(),
			detail: Some("Lua code block".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
				MarkupContent {
					kind: tower_lsp::lsp_types::MarkupKind::Markdown,
					value: "# Usage

Lua code block: ```
┃:lua
┃CONTENT
┃EOF```
Lua code block with properties: ```
┃:lua PROPERTIES
┃CONTENT
┃EOF```

Unless the `kernel` property is modified, lua is executed in the `main` kernel.
You can define and use multiple kernels to separate lua code.

`EOF` is the code block delimiter, it can be configured by setting the `delim` property.

# Examples

*Print hello world*: ```
┃:lua delim=@@
┃print(\"Hello, World\")
┃@@```
*Run in a different kernel*: ```
┃:lua kernel=foo
┃local result = bar()
┃EOF```

# Properties

 * `kernel` Lua kernel to use (defaults to **main**)
 * `delim` Delimiter to use for block (defaults to **EOF**)
 "
					.into(),
				},
			)),
			kind: Some(CompletionItemKind::FUNCTION),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
				"{}lua\n${{1:CONTENT}}\nEOF",
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
