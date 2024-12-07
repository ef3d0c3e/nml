use std::cell::Ref;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use serde::Deserialize;
use serde::Serialize;
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
pub struct CodeRangeParams {
	pub text_document: tower_lsp::lsp_types::TextDocumentIdentifier,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CodeRangeInfo {
	pub range: tower_lsp::lsp_types::Range,
	pub language: String,
	pub name: Option<String>,
}

/// Per file code ranges
#[derive(Debug)]
pub struct CodeRangeData {
	/// The ranges
	pub coderanges: RefCell<Vec<CodeRangeInfo>>,
}

impl CodeRangeData {
	pub fn new() -> Self {
		Self {
			coderanges: RefCell::new(vec![]),
		}
	}
}

/// Temporary data returned by [`Self::from_source_impl`]
#[derive(Debug)]
pub struct CodeRange<'a> {
	pub(self) coderanges: Ref<'a, CodeRangeData>,
	// The source used when resolving the parent source
	pub(self) original_source: Rc<dyn Source>,
	/// The resolved parent source
	pub(self) source: Rc<dyn Source>,
}

impl<'a> CodeRange<'a> {
	fn from_source_impl(
		source: Rc<dyn Source>,
		lsp: &'a Option<RefCell<LSPData>>,
		original_source: Rc<dyn Source>,
	) -> Option<Self> {
		if (source.name().starts_with(":LUA:") || source.name().starts_with(":VAR:"))
			&& source.downcast_ref::<VirtualSource>().is_some()
		{
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
			return Ref::filter_map(lsp.as_ref().unwrap().borrow(), |lsp: &LSPData| {
				lsp.coderanges.get(&(source.clone() as Rc<dyn Source>))
			})
			.ok()
			.map(|coderanges| Self {
				coderanges,
				source,
				original_source,
			});
		}
		None
	}

	pub fn from_source(source: Rc<dyn Source>, lsp: &'a Option<RefCell<LSPData>>) -> Option<Self> {
		if lsp.is_none() {
			return None;
		}
		Self::from_source_impl(source.clone(), lsp, source)
	}

	pub fn add(&self, range: Range<usize>, language: String, name: Option<String>) {
		let range = self.original_source.original_range(range.clone()).1;
		let mut cursor = LineCursor::new(self.source.clone(), OffsetEncoding::Utf8);

		cursor.move_to(range.start);
		let start_line = cursor.line;
		let start_char = cursor.line_pos;

		cursor.move_to(range.end);
		let end_line = cursor.line;
		let end_char = cursor.line_pos;

		self.coderanges.coderanges.borrow_mut().push(CodeRangeInfo {
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
			language,
			name,
		})
	}
}
