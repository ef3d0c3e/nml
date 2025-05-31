use tower_lsp::lsp_types::CompletionContext;
use tower_lsp::lsp_types::CompletionItem;
use tower_lsp::lsp_types::CompletionTriggerKind;

use crate::unit::translation::TranslationUnit;

/// Checks if completion context was triggered with sequence
pub fn context_triggered(context: &Option<CompletionContext>, sequence: &str) -> bool {
	let Some(context) = context else { return false };
	if context.trigger_kind != CompletionTriggerKind::TRIGGER_CHARACTER {
		return false;
	}
	let Some(trigger) = &context.trigger_character else {
		return false;
	};

	trigger == sequence
}

pub trait CompletionProvider: Send + Sync {
	fn trigger(&self) -> &'static [&'static str];

	/// Gets item related to the unit
	fn unit_items(&self, unit: &TranslationUnit, items: &mut Vec<CompletionItem>);

	/// Gets static items
	fn static_items(&self, context: &Option<CompletionContext>, items: &mut Vec<CompletionItem>);
}
