use std::sync::Arc;

use tower_lsp::lsp_types::CompletionItem;

use crate::{parser::source::Source, unit::translation::TranslationUnit};

pub trait CompletionProvider: Send + Sync {
	fn trigger(&self) -> &'static [&'static str];

	fn add_items(
		&self,
		unit: &TranslationUnit,
		items: &mut Vec<CompletionItem>,
	);
}
