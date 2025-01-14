use std::cell::Ref;
use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use tower_lsp::lsp_types::Position;

use crate::parser::source::LineCursor;
use crate::parser::source::OffsetEncoding;
use crate::parser::source::Source;
use crate::parser::source::SourceFile;
use crate::parser::source::SourcePosition;
use crate::parser::source::VirtualSource;

use super::data::LSPData;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConcealParams {
	pub text_document: tower_lsp::lsp_types::TextDocumentIdentifier,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConcealInfo {
	pub range: tower_lsp::lsp_types::Range,
	pub conceal_text: ConcealTarget,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ConcealTarget {
	/// Text to conceal with
	Text(String),
	/// Conceal using custom token
	Token {
		/// Name of the conceal token
		token: String,
		/// Parameters of the conceal token
		params: Value,
	},
}

/// Per file conceals
#[derive(Debug)]
pub struct ConcealsData {
	/// The conceals
	pub conceals: RefCell<Vec<ConcealInfo>>,
}

impl ConcealsData {
	pub fn new() -> Self {
		Self {
			conceals: RefCell::new(vec![]),
		}
	}
}

/// Temporary data returned by [`Self::from_source_impl`]
#[derive(Debug)]
pub struct Conceals<'a> {
	pub(self) conceals: Ref<'a, ConcealsData>,
	// The source used when resolving the parent source
	pub(self) original_source: Arc<dyn Source>,
	/// The resolved parent source
	pub(self) source: Arc<dyn Source>,
}

impl<'a> Conceals<'a> {
	fn from_source_impl(
		source: Arc<dyn Source>,
		lsp: &'a Option<RefCell<LSPData>>,
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
			return Ref::filter_map(lsp.as_ref().unwrap().borrow(), |lsp: &LSPData| {
				lsp.conceals.get(&(source.clone()))
			})
			.ok()
			.map(|conceals| Self {
				conceals,
				source,
				original_source,
			});
		}
		None
	}

	pub fn from_source(source: Arc<dyn Source>, lsp: &'a Option<RefCell<LSPData>>) -> Option<Self> {
		if lsp.is_none() {
			return None;
		}
		Self::from_source_impl(source.clone(), lsp, source)
	}

	pub fn add(&self, range: Range<usize>, text: ConcealTarget) {
		let range = self.original_source.original_range(range.clone()).1;
		let mut cursor = LineCursor::new(self.source.clone(), OffsetEncoding::Utf8);

		cursor.move_to(range.start);
		let line = cursor.line;
		let start_char = cursor.line_pos;

		cursor.move_to(range.end);
		assert_eq!(line, cursor.line);
		let end_char = cursor.line_pos;

		self.conceals.conceals.borrow_mut().push(ConcealInfo {
			range: tower_lsp::lsp_types::Range {
				start: Position {
					line: line as u32,
					character: start_char as u32,
				},
				end: Position {
					line: line as u32,
					character: end_char as u32,
				},
			},
			conceal_text: text,
		})
	}
}
