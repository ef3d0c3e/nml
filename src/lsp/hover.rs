use std::sync::Arc;

use parking_lot::RwLock;

use crate::parser::source::Source;
use crate::parser::source::SourceFile;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;

use super::data::LangServerData;

pub struct HoverRange {
	pub range: Token,
	pub content: String,
	//provider: Arc<dyn Fn() + Send + Sync>,
}

/// Per unit data
#[derive(Default)]
pub struct HoverData {
	pub hovers: RwLock<Vec<HoverRange>>,
}

pub struct Hover<'lsp> {
	hovers: &'lsp HoverData,
	// The source used when resolving the parent source
	original_source: Arc<dyn Source>,
	/// The resolved parent source
	source: Arc<dyn Source>,
}

impl<'lsp> Hover<'lsp> {
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
			return lsp.hovers.get(&source).map(|hovers| Self {
				hovers,
				source,
				original_source,
			});
		}
		None
	}

	pub fn from_source(source: Arc<dyn Source>, lsp: &'lsp LangServerData) -> Option<Self> {
		Self::from_source_impl(source.clone(), lsp, source)
	}

	pub fn add(&'lsp self, range: HoverRange) {
		self.hovers.hovers.write().push(range);
	}
}
