use std::cell::RefCell;
use std::sync::Arc;

use tower_lsp::lsp_types::Location;
use tower_lsp::lsp_types::Position;
use tower_lsp::lsp_types::Range;
use tower_lsp::lsp_types::Url;

use crate::parser::source::LineCursor;
use crate::parser::source::OffsetEncoding;
use crate::parser::source::Source;
use crate::parser::source::SourceFile;
use crate::parser::source::SourcePosition;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;

use super::data::LangServerData;

/// Per file definitions
#[derive(Debug)]
pub struct DefinitionData {
	/// The definitions
	pub definitions: RefCell<Vec<(Location, Range)>>,
}

impl DefinitionData {
	pub fn new() -> Self {
		Self {
			definitions: RefCell::new(vec![]),
		}
	}
}

fn from_source_impl<'lsp>(
	source: Arc<dyn Source>,
	target: &Token,
	lsp: &'lsp LangServerData,
	original: Token,
) {
	if (source.name().starts_with(":LUA:") || source.name().starts_with(":VAR:"))
		&& source.downcast_ref::<VirtualSource>().is_some()
	{
		return;
	}

	if let Some(location) = source
		.clone()
		.downcast_ref::<VirtualSource>()
		.map(|parent| parent.location())
		.unwrap_or(None)
	{
		from_source_impl(location.source(), target, lsp, original);
		return;
	} else if !source.downcast_ref::<SourceFile>().is_some() {
		return;
	}
	let Some(def_data) = lsp.definitions.get(&original.source()) else {
		return;
	};
	let mut db = def_data.definitions.borrow_mut();
	let token = original.source().original_range(original.range).range;

	// Resolve target
	let mut target_cursor = LineCursor::new(target.source(), OffsetEncoding::Utf16);
	let orignal_target = target.source().original_range(target.range.clone());
	target_cursor.move_to(orignal_target.range.start);
	let target_start = Position {
		line: target_cursor.line as u32,
		character: target_cursor.line_pos as u32,
	};
	target_cursor.move_to(orignal_target.range.end);
	let target_end = Position {
		line: target_cursor.line as u32,
		character: target_cursor.line_pos as u32,
	};

	// Resolve source
	let mut source_cursor = LineCursor::new(source, OffsetEncoding::Utf16);
	source_cursor.move_to(token.start);
	let source_start = Position {
		line: source_cursor.line as u32,
		character: source_cursor.line_pos as u32,
	};
	source_cursor.move_to(token.end);
	let source_end = Position {
		line: source_cursor.line as u32,
		character: source_cursor.line_pos as u32,
	};

	// Add definition
	let uri = if orignal_target.source().name().starts_with("file://") {
		Url::try_from(orignal_target.source().name().as_str()).unwrap()
	} else {
		let target_path = std::fs::canonicalize(orignal_target.source().name().as_str()).unwrap();
		Url::from_file_path(target_path).unwrap()
	};
	db.push((
		Location {
			uri,
			range: Range {
				start: target_start,
				end: target_end,
			},
		},
		Range {
			start: source_start,
			end: source_end,
		},
	))
}

pub fn from_source<'lsp>(source: Token, target: &Token, lsp: &'lsp LangServerData) {
	from_source_impl(source.source(), target, lsp, source)
}
