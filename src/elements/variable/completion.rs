use std::collections::HashSet;
use std::rc::Rc;

use tower_lsp::lsp_types::CompletionItem;
use tower_lsp::lsp_types::CompletionItemKind;
use tower_lsp::lsp_types::MarkupContent;

use crate::lsp::completion::CompletionProvider;
use crate::unit::translation::TranslationUnit;
use crate::unit::variable::Variable;
use crate::unit::variable::VariableName;

pub struct VariableCompletion;

impl VariableCompletion {
	fn get_documentation(name: &VariableName, var: &Rc<dyn Variable>) -> MarkupContent {
		MarkupContent {
			kind: tower_lsp::lsp_types::MarkupKind::Markdown,
			value: format!(
				"
# Value

```{}```

# Properties
 * **Type**: *{}*
 * **Definition**: {} ({}..{})
 * **Visibility**: *{}*
 * **Mutability**: *{}*",
				var.to_string(),
				var.variable_typename(),
				var.location().source().name(),
				var.location().range.start,
				var.location().range.end,
				var.visility(),
				var.mutability()
			),
		}
	}
}

impl CompletionProvider for VariableCompletion {
	fn trigger(&self) -> &'static [&'static str] {
		["%"].as_slice()
	}

	fn add_items(&self, unit: &TranslationUnit, items: &mut Vec<CompletionItem>) {
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
}
