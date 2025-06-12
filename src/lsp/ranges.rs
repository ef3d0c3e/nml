use std::sync::Arc;

use parking_lot::RwLock;

use crate::parser::source::Source;
use crate::parser::source::SourceFile;
use crate::parser::source::VirtualSource;

use super::data::LangServerData;

#[derive(Debug)]
pub enum CustomRange
{
	Lua,
}

/// Per unit data
#[derive(Default)]
pub struct RangeData {
	pub lua: RwLock<Vec<std::ops::Range<usize>>>,
}

pub struct Range<'lsp> {
	ranges: &'lsp RangeData,
	// The source used when resolving the parent source
	original_source: Arc<dyn Source>,
	/// The resolved parent source
	source: Arc<dyn Source>,
}

impl<'lsp> Range<'lsp> {
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
			return lsp.ranges.get(&source).map(|ranges| Self {
				ranges,
				source,
				original_source,
			});
		}
		None
	}

	pub fn from_source(source: Arc<dyn Source>, lsp: &'lsp LangServerData) -> Option<Self> {
		Self::from_source_impl(source.clone(), lsp, source)
	}

	pub fn add(&'lsp self, range: std::ops::Range<usize>, data: CustomRange) {
		self.ranges.lua.write().push(range);
	}
}
