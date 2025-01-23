use std::cell::Ref;
use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;
use tower_lsp::lsp_types::Position;

use crate::parser::source::LineCursor;
use crate::parser::source::OffsetEncoding;
use crate::parser::source::Source;
use crate::parser::source::SourceFile;
use crate::parser::source::SourcePosition;
use crate::parser::source::VirtualSource;

use super::data::LangServerData;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleParams {
	pub text_document: tower_lsp::lsp_types::TextDocumentIdentifier,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StyleInfo {
	pub range: tower_lsp::lsp_types::Range,
	pub style: Style,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum Style {
	/// Use a predefined highlight group from the editor
	Group(String),
}

/// Per file styles
#[derive(Debug)]
pub struct StylesData {
	/// The styles
	pub styles: RefCell<Vec<StyleInfo>>,
}

impl StylesData {
	pub fn new() -> Self {
		Self {
			styles: RefCell::new(vec![]),
		}
	}
}

/// Temporary data returned by [`Self::from_source_impl`]
#[derive(Debug)]
pub struct Styles<'a> {
	pub(self) styles: Ref<'a, StylesData>,
	// The source used when resolving the parent source
	pub(self) original_source: Arc<dyn Source>,
	/// The resolved parent source
	pub(self) source: Arc<dyn Source>,
}

impl<'a> Styles<'a> {
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
				lsp.styles.get(&(source.clone()))
			})
			.ok()
			.map(|styles| Self {
				styles,
				source,
				original_source,
			});
		}
		None
	}

	pub fn from_source(source: Arc<dyn Source>, lsp: &'a Option<RefCell<LangServerData>>) -> Option<Self> {
		if lsp.is_none() {
			return None;
		}
		Self::from_source_impl(source.clone(), lsp, source)
	}

	pub fn add(&self, range: Range<usize>, style: Style) {
		let range = self.original_source.original_range(range).1;
		let mut cursor = LineCursor::new(self.source.clone(), OffsetEncoding::Utf8);

		cursor.move_to(range.start);
		let start_line = cursor.line;
		let start_char = cursor.line_pos;

		cursor.move_to(range.end);
		let end_line = cursor.line;
		let end_char = cursor.line_pos;

		self.styles.styles.borrow_mut().push(StyleInfo {
			range: tower_lsp::lsp_types::Range {
				start: Position {
					line: start_line as u32,
					character: start_char as u32,
				},
				end: Position {
					line: end_line as u32,
					character: end_char as u32,
				},
			},
			style,
		})
	}
}
