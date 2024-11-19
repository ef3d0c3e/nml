use std::cell::RefCell;
use std::rc::Rc;

use tower_lsp::lsp_types::Location;
use tower_lsp::lsp_types::Position;
use tower_lsp::lsp_types::Range;
use tower_lsp::lsp_types::Url;

use crate::parser::source::LineCursor;
use crate::parser::source::Source;
use crate::parser::source::SourceFile;
use crate::parser::source::SourcePosition;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;

use super::data::LSPData;

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

fn from_source_impl(
	source: Rc<dyn Source>,
	target: &Token,
	lsp: &Option<RefCell<LSPData>>,
	original: Token,
) {
	if (source.name().starts_with(":LUA:") || source.name().starts_with(":VAR:"))
		&& source.downcast_ref::<VirtualSource>().is_some()
	{
		return;
	}

	if let Some(location) = source
		.clone()
		.downcast_rc::<VirtualSource>()
		.ok()
		.as_ref()
		.map(|parent| parent.location())
		.unwrap_or(None)
	{
		return from_source_impl(location.source(), target, lsp, original);
	} else if let Ok(sourcefile) = source.downcast_rc::<SourceFile>() {
		let borrow = lsp.as_ref().unwrap().borrow();
		let definitions = borrow.definitions.get(&original.source()).unwrap();
		let mut db = definitions.definitions.borrow_mut();
		{
			let token = original.source().original_range(original.range).1;

			// Resolve target
			let mut target_cursor = LineCursor::new(target.source());
			let orignal_target = target.source().original_range(target.range.clone());
			target_cursor.move_to(orignal_target.1.start);
			let target_start = Position {
				line: target_cursor.line as u32,
				character: target_cursor.line_pos as u32,
			};
			target_cursor.move_to(orignal_target.1.end);
			let target_end = Position {
				line: target_cursor.line as u32,
				character: target_cursor.line_pos as u32,
			};

			// Resolve source
			let mut source_cursor = LineCursor::new(sourcefile);
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
			let target_path = std::fs::canonicalize(orignal_target.0.name().as_str()).unwrap();
			let uri = Url::from_file_path(target_path).unwrap();
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
	}
}

pub fn from_source(source: Token, target: &Token, lsp: &Option<RefCell<LSPData>>) {
	if lsp.is_none() {
		return;
	}
	from_source_impl(source.source(), target, lsp, source)
}
