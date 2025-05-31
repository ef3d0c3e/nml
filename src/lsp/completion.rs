use std::{cell::{Ref, RefCell}, fmt::write, ops::Range, sync::Arc};

use tower_lsp::lsp_types::CompletionItem;

use crate::{parser::source::{LineCursor, OffsetEncoding, Source, SourceFile, SourcePosition, Token, VirtualSource}, unit::translation::TranslationUnit};

use super::{conceal::ConcealTarget, data::LangServerData};

/// A completion operation
pub struct CompleteRange {
	pub range: Token,
	pub apply: Arc<dyn Fn(&mut CompletionItem) + Send + Sync>,
}

/// Per file conceals
#[derive(Default)]
pub struct CompleteData {
	/// The completion data
	pub completes: RefCell<Vec<CompleteRange>>,
}

/// Temporary data returned by [`Self::from_source_impl`]
pub struct Completes<'lsp> {
	pub(self) completes: &'lsp CompleteData,
	// The source used when resolving the parent source
	pub(self) original_source: Arc<dyn Source>,
	/// The resolved parent source
	pub(self) source: Arc<dyn Source>,
}

impl<'lsp> Completes<'lsp> {
	fn from_source_impl(
		source: Arc<dyn Source>,
		lsp: &'lsp LangServerData,
		original_source: Arc<dyn Source>,
	) -> Option<Self> {
		if (source.name().starts_with(":LUA:") || source.name().starts_with(":VAR:"))
			&& source.downcast_ref::<VirtualSource>().is_some()
		{
			return None;
		}

		if let Some(location) = source
			.clone()
			.downcast_ref::<VirtualSource>()
			.map(|parent| parent.location())
			.unwrap_or(None)
		{
			return Self::from_source_impl(location.source(), lsp, original_source);
		} else if source.downcast_ref::<SourceFile>().is_some() {
			return lsp.completes.get(&source)
			.map(|completes| Self {
				completes,
				source,
				original_source,
			});
		}
		None
	}

	pub fn from_source(source: Arc<dyn Source>, lsp: &'lsp LangServerData) -> Option<Self> {
		Self::from_source_impl(source.clone(), lsp, source)
	}

	pub fn add(&self, range: CompleteRange) {
		self.completes.completes.borrow_mut().push(range)
	}
}

pub trait CompletionProvider: Send + Sync {
	fn trigger(&self) -> &'static [&'static str];

	fn add_items(
		&self,
		unit: &TranslationUnit,
		items: &mut Vec<CompletionItem>,
	);
}
