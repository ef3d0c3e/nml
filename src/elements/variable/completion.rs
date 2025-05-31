use std::collections::HashSet;
use std::rc::Rc;

use tower_lsp::lsp_types::CompletionContext;
use tower_lsp::lsp_types::CompletionItem;
use tower_lsp::lsp_types::CompletionItemKind;
use tower_lsp::lsp_types::InsertTextFormat;
use tower_lsp::lsp_types::MarkupContent;
use tower_lsp::lsp_types::SignatureHelp;

use crate::lsp::completion::context_triggered;
use crate::lsp::completion::CompletionProvider;
use crate::unit::translation::TranslationUnit;
use crate::unit::variable::Variable;
use crate::unit::variable::VariableName;

pub struct VariableCompletion;

impl VariableCompletion {
	fn get_documentation(_name: &VariableName, var: &Rc<dyn Variable>) -> MarkupContent {
		let range = if var.location().end() != 0 {
			format!(" ({}..{})", var.location().start(), var.location().end())
		} else {
			"".into()
		};
		MarkupContent {
			kind: tower_lsp::lsp_types::MarkupKind::Markdown,
			value: format!(
"# Value

```{0}```

# Properties
 * **Type**: *{1}*
 * **Definition**: [{2}](){range}
 * **Visibility**: *{3}*
 * **Mutability**: *{4}*",
				var.to_string(),
				var.variable_typename(),
				var.location().source().name(),
				var.visility(),
				var.mutability()
			),
		}
	}
}

impl CompletionProvider for VariableCompletion {
	fn trigger(&self) -> &'static [&'static str] {
		["%", ":"].as_slice()
	}

	fn unit_items(&self, unit: &TranslationUnit, items: &mut Vec<CompletionItem>) {
		struct Item(CompletionItem);
		impl std::hash::Hash for Item {
			fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
				state.write(self.0.label.as_bytes());
			}
		}
		impl PartialEq for Item {
			fn eq(&self, other: &Self) -> bool {
				self.0.label == other.0.label
			}
		}

		impl Eq for Item {}
		let mut set = HashSet::new();
		let mut scope = unit.get_scope().clone();
		loop {
			let borrow = scope.borrow();
			for (name, var) in &borrow.variables {
				set.insert(Item(CompletionItem {
					label: name.to_string(),
					detail: Some(format!("Variable {name}")),
					documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
						Self::get_documentation(name, var),
					)),
					kind: Some(CompletionItemKind::VARIABLE),
					..CompletionItem::default()
				}));
			}

			let Some(parent) = borrow.parent().clone() else {
				break;
			};
			drop(borrow);
			scope = parent;
		}
		set.drain().for_each(|item| items.push(item.0));
	}

	fn static_items(&self, context: &Option<CompletionContext>, items: &mut Vec<CompletionItem>) {
		// :set
		items.push(CompletionItem {
			label: ":set ".to_string(),
			detail: Some("Set a variable".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(MarkupContent {
				kind: tower_lsp::lsp_types::MarkupKind::Markdown,
				value:
"# Usage
				
`:set VARNAME = VALUE`

Set variables are available through the current file. They are not inherited by
`:import`-ing.

# Examples

`:set foo = \"Hello\"` *sets variable **foo** with string value **Hello***
`:set bar = false` *sets variable **bar** with integer value **0***
`:set baz = {{ [My **E-Mail** address](mailto://me@mail.org) }}` *sets variable **baz** with string value*
`:set x = 64` *sets variable `x` with integer value **64***

# Variable types

 * `String`: delimited by `\"`, `'`, `\"\"\"`, `'''``.
	Additionally strings variables can be enclosed between `{{` and `}}`.
 * `Integer`: no delimiters, a 64-bit signed integer.
	Additionally `true` can be used for `1` and `false` for `0`.

# See also

 * `:export` *export a variable*".into()
			})),
			kind: Some(CompletionItemKind::VALUE),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!("{}set ${{1:VARIABLE}} = ${{2:VALUE}}", if context_triggered(context, ":") { "" } else { ":" })),
			..CompletionItem::default()
		});

		// :export
		items.push(CompletionItem {
			label: ":export ".to_string(),
			detail: Some("Export a variable".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(MarkupContent {
				kind: tower_lsp::lsp_types::MarkupKind::Markdown,
				value:
"# Usage
				
`:export VARNAME = VALUE`

Exported variables will be available to any subsequent file that `:import`s the file where they are defined.

# Examples

`:export foo = \"Hello\"` *exports variable **foo** with string value **Hello***
`:export bar = false` *exports variable **bar** with integer value **0***
`:export baz = {{ [My **E-Mail** address](mailto://me@mail.org) }}` *exports variable **baz** with string value*
`:export x = 64` *exports variable `x` with integer value **64***

# Variable types

 * `String`: delimited by `\"`, `'`, `\"\"\"`, `'''``
	Additionally strings variables can be enclosed between `{{` and `}}`.
 * `Integer`: no delimiters, a 64-bit signed integer.
	Additionally `true` can be used for `1` and `false` for `0`.

# See also

 * `:set` *set a variable*".into()
			})),
			kind: Some(CompletionItemKind::VALUE),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!("{}export ${{1:VARIABLE}} = ${{2:VALUE}}", if context_triggered(context, ":") { "" } else { ":" })),
			..CompletionItem::default()
		});
	}
}
