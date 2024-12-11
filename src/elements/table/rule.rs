use std::any::Any;
use std::collections::HashMap;
use std::fmt::Display;
use std::ops::Range;

use crate::document::document::Document;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::reports::macros::*;
use crate::parser::reports::Report;
use crate::parser::reports::*;
use crate::parser::rule::Rule;
use crate::parser::source::Cursor;
use crate::parser::util::escape_source;
use crate::report_err;
use ariadne::Fmt;
use document::element::ElemKind;
use document::element::Element;
use elements::block::elem::Block;
use elements::list::elem::ListEntry;
use elements::list::elem::ListMarker;
use elements::paragraph::elem::Paragraph;
use elements::text::elem::Text;
use parser::property::PropertyMap;
use parser::source::Token;
use regex::Regex;

use super::elem::Cell;
use super::elem::CellData;
use super::elem::CellProperties;
use super::elem::Table;

#[derive(Clone, Copy)]
struct GridPosition(usize, usize);

impl Display for GridPosition {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "({}, {})", self.0, self.1)
	}
}

/// Holds all cells that span over multiple cells
#[derive(Default)]
struct Overlaps {
	// Holds top-left, bottom-right
	cells: Vec<(Range<usize>, GridPosition, GridPosition)>,
}

impl Overlaps {
	pub fn push(&mut self, span: (Range<usize>, GridPosition, GridPosition)) {
		self.cells.push(span)
	}

	/// Gets whether a given rectangle is occupied by another cell
	pub fn is_occupied(
		&self,
		pos: &GridPosition,
		size: &GridPosition,
	) -> Option<(Range<usize>, GridPosition, GridPosition)> {
		for (range, tl, br) in &self.cells {
			if tl.0 < pos.0 + size.0 && br.0 > pos.0 && tl.1 < pos.1 + size.1 && br.1 > pos.1 {
				return Some((
					range.to_owned(),
					tl.to_owned(),
					GridPosition(br.0 - tl.0, br.1 - tl.1),
				));
			}
		}
		None
	}
}

struct TableState {
	pub first_row: bool,
	pub current_row: Range<usize>,
	pub overlaps: Overlaps,
}

impl TableState {
	pub fn new(row: Range<usize>) -> Self {
		Self {
			first_row: true,
			current_row: row,
			overlaps: Overlaps::default(),
		}
	}
}

fn parse_properties(
	properties: &PropertyMap,
	reports: &mut Vec<Report>,
	state: &ParserState,
) -> Option<CellProperties> {
	let (vspan, hspan) =
		match (
			properties.get(reports, "vspan", |_, value| {
				let result = value.value.parse::<usize>().map_err(|e| {
					format!("Failed to parse `{}` as positive integer: {e}", value.value)
				});
				if let Ok(val) = result {
					if val == 0 {
						return Err(format!(
							"{0} may not be 0",
							"hspan".fg(state.parser.colors().info)
						));
					}
				}
				result
			}),
			properties.get(reports, "hspan", |_, value| {
				let result = value.value.parse::<usize>().map_err(|e| {
					format!("Failed to parse `{}` as positive integer: {e}", value.value)
				});
				if let Ok(val) = result {
					if val == 0 {
						return Err(format!(
							"{0} may not be 0",
							"hspan".fg(state.parser.colors().info)
						));
					}
				}
				result
			}),
		) {
			(Some(vspan), Some(hspan)) => (vspan, hspan),
			_ => return None,
		};

	Some(CellProperties { vspan, hspan })
}

#[auto_registry::auto_registry(registry = "rules")]
pub struct TableRule {
	properties: PropertyParser,
	re: Regex,
	cell_re: Regex,
}

impl Default for TableRule {
	fn default() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"vspan".to_string(),
			Property::new("Cell vertial span".to_string(), Some("1".into())),
		);
		props.insert(
			"hspan".to_string(),
			Property::new("Cell horizontal span".to_string(), Some("1".into())),
		);

		Self {
			properties: PropertyParser { properties: props },
			re: Regex::new(r"(:?^|\n)\|").unwrap(),
			cell_re: Regex::new(
				r"\|(?:[^\S\r\n]*:([^\n](?:\\[^\n]|[^\\\\])*?):)?([^\n](?:\\[^\n]|[^\n\\\\])*?)\|",
			)
			.unwrap(),
		}
	}
}

impl Rule for TableRule {
	fn name(&self) -> &'static str { "Table" }

	fn previous(&self) -> Option<&'static str> { Some("Toc") }

	fn next_match(
		&self,
		mode: &ParseMode,
		_state: &ParserState,
		cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any>)> {
		if mode.paragraph_only {
			return None;
		}
		self.re
			.find_at(cursor.source.content(), cursor.pos)
			.map(|m| (m.start(), Box::new(()) as Box<dyn Any>))
	}

	fn on_match<'a>(
		&self,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
		cursor: Cursor,
		match_data: Box<dyn Any>,
	) -> (Cursor, Vec<Report>) {
		let mut reports = vec![];
		let mut end_cursor = cursor.clone();
		end_cursor.pos += 1;

		let mut cell_pos = GridPosition(0, 0);
		let mut dimensions = cell_pos;

		let mut table_state = TableState::new(cursor.pos..cursor.pos + 1);
		let mut cells = Vec::<Cell>::new();
		while let Some(captures) = self
			.cell_re
			.captures_at(end_cursor.source.content(), end_cursor.pos)
		{
			let range = captures.get(0).unwrap().range();
			if range.start != end_cursor.pos {
				break;
			}

			// Get properties
			let prop_source = escape_source(
				cursor.source.clone(),
				captures.get(1).map_or(0..0, |m| m.range()),
				"Cell Properties".into(),
				'\\',
				":",
			);
			let properties = match self.properties.parse(
				"Block Quote",
				&mut reports,
				state,
				prop_source.clone().into(),
			) {
				Some(props) => props,
				None => return (end_cursor, reports),
			};
			let cell_properties = match parse_properties(&properties, &mut reports, state) {
				Some(props) => props,
				None => return (end_cursor, reports),
			};

			// Parse cell
			let mut cell_source = escape_source(
				end_cursor.source.clone(),
				captures.get(2).unwrap().range(),
				format!(":Cell:({}, {})", cell_pos.0, cell_pos.1),
				'\\',
				"|",
			);
			// Check for overlaps in case the cell is not empty
			if !prop_source.content().is_empty() || cell_source.content().trim_start().len() != 0 {
				if let Some((overlap_range, pos, size)) = table_state.overlaps.is_occupied(
					&cell_pos,
					&GridPosition(cell_properties.hspan, cell_properties.vspan),
				) {
					report_err!(
						&mut reports,
						cursor.source.clone(),
						"Invalid Table Cell".into(),
						span(
							range.start + 1..range.end - 1,
							format!(
								"Cell {} is already occupied by cell {}",
								cell_pos.fg(state.parser.colors().info),
								pos.fg(state.parser.colors().info)
							)
						),
						span(overlap_range, format!("Occupied by this cell")),
						note(format!(
							"Cell {} spans a {} rectangle",
							pos.fg(state.parser.colors().info),
							size
						))
					);
				}
			}

			// Parse content
			let parsed_cell = state.with_state(|new_state| {
				new_state
					.parser
					.parse(
						new_state,
						cell_source.clone(),
						Some(document),
						ParseMode::default(),
					)
					.0
			});
			let mut parsed_content: Vec<Box<dyn Element>> = vec![];
			for mut elem in parsed_cell.content().borrow_mut().drain(..) {
				if let Some(paragraph) = elem.downcast_mut::<Paragraph>() {
					// Insert space between paragraphs
					if let Some(last) = parsed_content.last() {
						if last.kind() == ElemKind::Inline {
							parsed_content.push(Box::new(Text {
								location: Token::new(
									last.location().end()..last.location().end(),
									last.location().source(),
								),
								content: " ".to_string(),
							}) as Box<dyn Element>);
						}
					}
					parsed_content.extend(std::mem::take(&mut paragraph.content));
				} else if elem.downcast_ref::<Block>().is_some()
					|| elem.downcast_ref::<ListEntry>().is_some()
					|| elem.downcast_ref::<ListMarker>().is_some()
				{
					parsed_content.push(elem);
				} else {
					report_err!(
						&mut reports,
						end_cursor.source.clone(),
						"Unable to Parse Table Cell".into(),
						span(
							captures.get(2).unwrap().range(),
							"Cells may only contain paragraphs, lists or blocks".into()
						)
					);
					return (end_cursor, reports);
				}
			}

			// If empty, insert reference to owning cell
			if prop_source.content().is_empty() && cell_source.content().trim_start().len() == 0 {
				if let Some((overlap_range, pos, size)) = table_state
					.overlaps
					.is_occupied(&cell_pos, &GridPosition(1, 1))
				{
					for _i in 0..size.0 {
						cells.push(Cell::Reference(pos.0 + pos.1 * dimensions.1));
					}
					cell_pos.0 += size.0 - 1;
				} else {
					cells.push(Cell::Owning(CellData {
						location: Token::new(
							captures.get(2).unwrap().range(),
							end_cursor.source.clone(),
						),
						content: parsed_content,
						properties: cell_properties.clone(),
					}))
				}
			}
			// Otherwise insert owning cell
			else {
				cells.push(Cell::Owning(CellData {
					location: Token::new(
						captures.get(2).unwrap().range(),
						end_cursor.source.clone(),
					),
					content: parsed_content,
					properties: cell_properties.clone(),
				}));

				for _i in 0..cell_properties.hspan - 1 {
					cells.push(Cell::Reference(cell_pos.0 + cell_pos.1 * dimensions.1));
				}
			}

			// Insert cell to the overlaps if not a (1, 1) cell
			if cell_properties.vspan != 1 || cell_properties.hspan != 1 {
				table_state.overlaps.push((
					range.start + 1..range.end - 1,
					GridPosition(cell_pos.0, cell_pos.1),
					GridPosition(
						cell_pos.0 + cell_properties.hspan,
						cell_pos.1 + cell_properties.vspan,
					),
				));
			}

			// Update width
			cell_pos.0 += cell_properties.hspan;
			if table_state.first_row {
				dimensions.0 += cell_properties.hspan;
			}

			end_cursor.pos = range.end - 1;
			if end_cursor.source.content().as_bytes()[end_cursor.pos + 1] != b'\n' {
				// Next column
				if cell_pos.0 > dimensions.0 {
					dimensions.0 = cell_pos.0;
				}

				table_state.current_row = table_state.current_row.start..end_cursor.pos;
			} else {
				table_state.current_row = table_state.current_row.start..end_cursor.pos + 1;
				// Next row
				if !table_state.first_row && cell_pos.0 != dimensions.0 {
					report_err!(
						&mut reports,
						cursor.source.clone(),
						"Invalid Table Row".into(),
						span(
							table_state.current_row.clone(),
							format!(
								"Row has horizontal width {}, table requires {}",
								cell_pos.0.fg(state.parser.colors().info),
								dimensions.0.fg(state.parser.colors().info)
							)
						)
					);
				}

				end_cursor.pos += 2;
				table_state.current_row = end_cursor.pos..end_cursor.pos + 1;

				table_state.first_row = false;

				cell_pos.1 += 1;
				dimensions.1 += 1;
				cell_pos.0 = 0;
			}
		}
		// TODO: Check if there are multi-cell cells whose height leads after the table end

		state.push(
			document,
			Box::new(Table {
				location: Token::new(cursor.pos..end_cursor.pos, cursor.source.clone()),
				size: (dimensions.0, dimensions.1),
				// TODO
				columns: vec![],
				rows: vec![],
				data: cells,
			}),
		);

		(end_cursor, reports)
	}
}
