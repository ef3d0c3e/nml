use std::{cell::{Ref, RefCell}, rc::Rc};

use tower_lsp::lsp_types::InlayHint;

use crate::parser::source::{LineCursor, Source, SourceFile, VirtualSource};

use super::data::LSPData;

/// Per file hints
#[derive(Debug)]
pub struct HintsData
{
	/// The current cursor
	cursor: RefCell<LineCursor>,

	/// The hints
	pub hints: RefCell<Vec<InlayHint>>,
}

impl HintsData {
	pub fn new(source: Rc<dyn Source>) -> Self {
		Self {
			cursor: RefCell::new(LineCursor::new(source)),
			hints: RefCell::new(vec![]),
		}
	}
}

#[derive(Debug)]
pub struct Hints<'a> {
	pub(self) hints: Ref<'a, HintsData>,
	// The source used when resolving the parent source
	pub(self) original_source: Rc<dyn Source>,
	/// The resolved parent source
	pub(self) source: Rc<dyn Source>,
}

impl<'a> Hints<'a> {
	fn from_source_impl(
		source: Rc<dyn Source>,
		lsp: &'a Option<RefCell<LSPData>>,
		original_source: Rc<dyn Source>,
	) -> Option<Self> {
		if source.name().starts_with(":LUA:") && source.downcast_ref::<VirtualSource>().is_some() {
			return None;
		}

		if let Some(location) = source
			.clone()
			.downcast_rc::<VirtualSource>()
			.ok()
			.as_ref()
			.map(|parent| parent.location())
			.unwrap_or(None)
		{
			return Self::from_source_impl(location.source(), lsp, original_source);
		} else if let Ok(source) = source.clone().downcast_rc::<SourceFile>() {
			return Ref::filter_map(
				lsp.as_ref().unwrap().borrow(),
				|lsp: &LSPData| {
					lsp.inlay_hints.get(&(source.clone() as Rc<dyn Source>))
				},
			)
			.ok()
			.map(|hints| {
				Self {
					hints,
					source,
					original_source,
				}
			});
		}
		None
	}

	pub fn from_source(
		source: Rc<dyn Source>,
		lsp: &'a Option<RefCell<LSPData>>,
	) -> Option<Self> {
		if lsp.is_none() {
			return None;
		}
		Self::from_source_impl(source.clone(), lsp, source)
	}

	pub fn add(&self, position: usize, label: String)
	{
		let mut cursor = self.hints.cursor.borrow_mut();
		cursor.move_to(position);

		self.hints.hints.borrow_mut().push(InlayHint {
			position: tower_lsp::lsp_types::Position { line: cursor.line as u32, character: cursor.line_pos as u32 },
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
