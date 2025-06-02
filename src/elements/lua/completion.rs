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
				context_triggered(context, ":").then_some("").unwrap_or(":")
			)),
			..CompletionItem::default()
		});

		// {:lua :}
		items.push(CompletionItem {
			label: "{:lua:}".to_string(),
			detail: Some("Lua inline code".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
				MarkupContent {
					kind: tower_lsp::lsp_types::MarkupKind::Markdown,
					value: "# Usage

Inline Lua:
`{:lua `**CONTENT**`}`
Inline Lua with properties:
`{:lua[`**PROPERTIES**`] `**CONTENT**`}`
Inline Lua with custom kind:
`{:lua`**KIND**` `**CONTENT**`}`
Inline Lua with custom kind and properties:
`{:lua[`**PROPERTIES**`]`**KIND**` `**CONTENT**`}`

# Examples

 * `{:lua' \"Hello World\"}` *create a text element with `Hello World` as content*
 * `{:lua! \"**bar**\"}` *parse `**bar**` and create an element from the result*
 * `{:lua[kernel=foo] print(\"bar\"):}` *evaluates `print(\"bar\")` in lua kernel **foo***

# Kind

The inline lua element supports the following kinds:
 * `(None)`: Only evaluate lua, discard the result.
 * `'`: Evaluate and displays the result as text.
 * `!`: Evaluate and parse the result

# Properties

 * `kernel` Lua kernel to use (defaults to **main**)
 "
					.into(),
				},
			)),
			kind: Some(CompletionItemKind::FUNCTION),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
				"{{:lua ${{1:CONTENT}}:}}",
			)),
			..CompletionItem::default()
		});
	}
}
