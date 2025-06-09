use tower_lsp::lsp_types::CompletionContext;
use tower_lsp::lsp_types::CompletionItem;
use tower_lsp::lsp_types::CompletionItemKind;
use tower_lsp::lsp_types::InsertTextFormat;
use tower_lsp::lsp_types::MarkupContent;

use crate::lsp::completion::context_triggered;
use crate::lsp::completion::CompletionProvider;
use crate::unit::translation::TranslationUnit;

pub struct LatexCompletion;

impl CompletionProvider for LatexCompletion {
	fn trigger(&self) -> &'static [&'static str] {
		["$"].as_slice()
	}

	fn unit_items(&self, _unit: &TranslationUnit, _items: &mut Vec<CompletionItem>) {}

	fn static_items(&self, context: &Option<CompletionContext>, items: &mut Vec<CompletionItem>) {
		// $LaTeX$
		items.push(CompletionItem {
			label: "$LaTeX$".to_string(),
			detail: Some("Mathmode LaTeX".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
				MarkupContent {
					kind: tower_lsp::lsp_types::MarkupKind::Markdown,
					value: "# Usage

`$LaTeX$` *render LaTeX content in mathmode, displays inline*
`$[kind=block]LaTeX$` *render LaTeX content in mathmode, displays as a block*

# Examples

 * `$1+1=2$` *display **1+1=2** as an inline element*
 * `$[kind=block]\\LaTeX$` *display **LaTeX** as a block element*
 * `$[env=custom]\\sum_{k=0}^n \\frac{1}{k^2}$` *render using LaTeX environment **custom***

# Properties

 * `env` LaTeX environment to use (defaults to **main**)
 * `kind` Display kind of the LaTeX element (defaults to **inline**)
		- **inline** Element displays inline
		- **block** Element displays as a block
 * `caption` Alternate text display for the LaTeX element (defaults to none)

# See also

 * `$|LaTeX|$` *normal mode LaTeX*

"
					.into(),
				},
			)),
			kind: Some(CompletionItemKind::SNIPPET),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
				"{}${{1:LATEX}}$",
				if context_triggered(context, "$") {
					""
				} else {
					"$"
				}
			)),
			..CompletionItem::default()
		});

		// $|LaTeX|$
		items.push(CompletionItem {
			label: "$|LaTeX|$".to_string(),
			detail: Some("Normal LaTeX".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
				MarkupContent {
					kind: tower_lsp::lsp_types::MarkupKind::Markdown,
					value: "# Usage

`$|LaTeX|$` *render LaTeX content, displays as a block*
`$|[kind=inline]|LaTeX|$` *render LaTeX content, displays inline*

# Examples

 * `$|\\textit{italic}|$` *display **italic** as a block*
 * `$|[kind=inline]\\LaTeX|$` *display **LaTeX** inline*
 * `$|[env=custom]\\textbb{Bold}|$` *render using LaTeX environment **custom***

# Properties

 * `env` LaTeX environment to use (defaults to **main**)
 * `kind` Display kind of the LaTeX element (defaults to **block**)
		- **inline** Element displays inline
		- **block** Element displays as a block
 * `caption` Alternate text display for the LaTeX element (defaults to none)

# See also

 * `$LaTeX$` *mathmode LaTeX*

"
					.into(),
				},
			)),
			kind: Some(CompletionItemKind::SNIPPET),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
				"{}|${{1:LATEX}}|$",
				if context_triggered(context, "$") {
					""
				} else {
					"$"
				}
			)),
			..CompletionItem::default()
		});
	}
}
