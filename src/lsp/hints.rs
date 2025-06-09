use std::cell::Ref;
use std::cell::RefCell;
use std::sync::Arc;

use tower_lsp::lsp_types::InlayHint;

use crate::parser::source::LineCursor;
use crate::parser::source::OffsetEncoding;
use crate::parser::source::Source;
use crate::parser::source::SourceFile;
use crate::parser::source::SourcePosition;
use crate::parser::source::VirtualSource;

use super::data::LangServerData;

/// Per file hints
#[derive(Debug)]
pub struct HintsData {
	/// The current cursor
	cursor: RefCell<LineCursor>,

	/// The hints
	pub hints: RefCell<Vec<InlayHint>>,
}

impl HintsData {
	pub fn new(source: Arc<dyn Source>) -> Self {
		Self {
			cursor: RefCell::new(LineCursor::new(source, OffsetEncoding::Utf16)),
			hints: RefCell::new(vec![]),
		}
	}
}

/// Temporary data returned by [`Self::from_source_impl`]
#[derive(Debug)]
pub struct Hints<'a> {
	pub(self) hints: Ref<'a, HintsData>,
	// The source used when resolving the parent source
	pub(self) original_source: Arc<dyn Source>,
	/// The resolved parent source
	pub(self) source: Arc<dyn Source>,
}

impl<'a> Hints<'a> {
	fn from_source_impl(
		source: Arc<dyn Source>,
		lsp: &'a Option<RefCell<LangServerData>>,
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
			return Ref::filter_map(lsp.as_ref().unwrap().borrow(), |lsp: &LangServerData| {
				lsp.inlay_hints.get(&(source.clone()))
			})
			.ok()
			.map(|hints| Self {
				hints,
				source,
				original_source,
			});
		}
		None
	}

	pub fn from_source(
		source: Arc<dyn Source>,
		lsp: &'a Option<RefCell<LangServerData>>,
	) -> Option<Self> {
		if lsp.is_none() {
			return None;
		}
		Self::from_source_impl(source.clone(), lsp, source)
	}

	pub fn add(&self, position: usize, label: String) {
		let position = self.original_source.original_position(position).1;
		let mut cursor = self.hints.cursor.borrow_mut();
		cursor.move_to(position);

		self.hints.hints.borrow_mut().push(InlayHint {
			position: tower_lsp::lsp_types::Position {
				line: cursor.line as u32,
				character: cursor.line_pos as u32,
			},
			label: tower_lsp::lsp_types::InlayHintLabel::String(label),
			kind: Some(tower_lsp::lsp_types::InlayHintKind::PARAMETER),
			text_edits: None,
			tooltip: None,
			padding_left: None,
			padding_right: None,
			data: None,
		})
	}
}
