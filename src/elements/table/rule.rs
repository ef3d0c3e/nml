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
use document::references::validate_refname;
use elements::block::elem::Block;
use elements::list::elem::ListEntry;
use elements::list::elem::ListMarker;
use elements::paragraph::elem::Paragraph;
use elements::text::elem::Text;
use lsp::semantic::Semantics;
use lua::kernel::CTX;
use parser::property::PropertyMap;
use parser::source::Token;
use regex::Regex;

use super::elem::Align;
use super::elem::Cell;
use super::elem::CellData;
use super::elem::CellProperties;
use super::elem::ColumnProperties;
use super::elem::RowProperties;
use super::elem::Table;
use super::elem::TableProperties;

/// Represents a position inside the table grid
#[derive(Clone, Copy)]
struct GridPosition(usize, usize);

impl Display for GridPosition {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "({}, {})", self.0, self.1)
	}
}

/// Holds all cells that span over multiple cells e.g a rectangle.
///
/// Cells with size (1, 1) (default) do not need to be added to this list.
#[derive(Default)]
struct Overlaps {
	// Holds top-left, bottom-right
	cells: Vec<(Range<usize>, GridPosition, GridPosition)>,
}

impl Overlaps {
	/// Adds a cell rectangle to the overlap list
	pub fn push(&mut self, span: (Range<usize>, GridPosition, GridPosition)) {
		self.cells.push(span)
	}

	/// Gets whether a given cell rectangle overlaps with another cell rectangle
	///
	/// # Return
	///
	/// The range of the original overlapping cell as well as it's position and dimensions.
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

/// Represents state of the table parser
struct TableState {
	/// Whether the currently parsed row is the first table row
	pub first_row: bool,
	/// Range of the current row, for reporting
	pub current_row: Range<usize>,
	/// Stores rectangle cells that needs to be checked against for overlaps
	pub overlaps: Overlaps,
	/// Properties for columns
	pub columns: Vec<Option<ColumnProperties>>,
	/// Properties for rows
	pub rows: Vec<Option<RowProperties>>,
	/// Properties for the table
	pub properties: TableProperties,
}

impl TableState {
	pub fn new(row: Range<usize>) -> Self {
		Self {
			first_row: true,
			current_row: row,
			overlaps: Overlaps::default(),
			columns: vec![],
			rows: vec![],
			properties: TableProperties::default(),
		}
	}
}

fn parse_properties(
	properties: &PropertyMap,
	reports: &mut Vec<Report>,
	state: &ParserState,
	position: &GridPosition,
	table_state: &mut TableState,
) -> Option<CellProperties> {
	let parse_span = |reports: &mut Vec<Report>, key: &'static str| {
		properties.get_opt(reports, key, |_, value| {
			let result = value
				.value
				.parse::<usize>()
				.map_err(|e| format!("Failed to parse `{}` as positive integer: {e}", value.value));
			if let Ok(val) = result {
				if val == 0 {
					return Err(format!(
						"{0} may not be 0",
						key.fg(state.parser.colors().info)
					));
				}
			}
			result
		})
	};

	// Cells
	let hspan = match parse_span(reports, "hspan") {
		Some(span) => span,
		None => return None,
	};
	let vspan = match parse_span(reports, "vspan") {
		Some(span) => span,
		None => return None,
	};
	let align = match properties.get_opt(reports, "align", |_, value| Align::try_from(&value.value))
	{
		Some(align) => align,
		None => return None,
	};
	let cell_properties = CellProperties {
		hspan,
		vspan,
		align,
		borders: [None; 4],
	};

	// Row
	let mut row = &mut table_state.rows[position.1];
	// Row Align
	match (
		&mut row,
		match properties.get_opt(reports, "ralign", |_, value| {
			Align::try_from(&value.value).map(|val| (value.value_range.clone(), val))
		}) {
			Some(align) => align,
			None => return None,
		},
	) {
		(Some(row), Some(align)) => {
			if row.align.is_some() {
				report_err!(
					reports,
					properties.token.source(),
					"Duplicate row property".into(),
					span(
						align.0,
						format!(
							"Property {} is already specified",
							"ralign".fg(state.parser.colors().info)
						)
					),
				);
				return None;
			}
			row.align.replace(align.1);
		}
		(None, Some(align)) => {
			row.replace(RowProperties {
				vspan: None,
				align: Some(align.1),
				borders: [None; 4],
			});
		}
		_ => {}
	}
	// Row vspan
	if let Some(span) = match parse_span(reports, "rvspan") {
		Some(span) => span,
		None => return None,
	} {
		match row {
			Some(row) => row.vspan = Some(span),
			None => {
				row.replace(RowProperties {
					vspan: Some(span),
					align: None,
					borders: [None; 4],
				});
			}
		}
	}

	// Column
	let col = &mut table_state.columns[position.0];
	// Column hspan
	if let Some(span) = match parse_span(reports, "chspan") {
		Some(span) => span,
		None => return None,
	} {
		match col {
			Some(col) => col.hspan = Some(span),
			None => {
				col.replace(ColumnProperties {
					hspan: Some(span),
					borders: [None; 4],
				});
			}
		}
	}

	// Table align
	match match properties.get_opt(reports, "talign", |_, value| {
		Align::try_from(&value.value).map(|val| (value.value_range.clone(), val))
	}) {
		Some(align) => align,
		None => return None,
	} {
		Some(align) => {
			if table_state.properties.align.is_some() {
				report_err!(
					reports,
					properties.token.source(),
					"Duplicate table property".into(),
					span(
						align.0,
						format!(
							"Property {} is already specified",
							"talign".fg(state.parser.colors().info)
						)
					),
				);
				return None;
			}
			table_state.properties.align.replace(align.1);
		}
		_ => {}
	}

	Some(cell_properties)
}

#[auto_registry::auto_registry(registry = "rules")]
pub struct TableRule {
	properties: PropertyParser,
	cell_properties: PropertyParser,
	re: Regex,
	cell_re: Regex,
}

impl Default for TableRule {
	fn default() -> Self {
		// Table properties
		let mut props = HashMap::new();
		props.insert(
			"export_as".to_string(),
			Property::new("Export the table to LUA".to_string(), None),
		);

		// Cell properties
		let mut cell_props = HashMap::new();
		cell_props.insert(
			"vspan".to_string(),
			Property::new("Cell vertial span".to_string(), None),
		);
		cell_props.insert(
			"hspan".to_string(),
			Property::new("Cell horizontal span".to_string(), None),
		);
		cell_props.insert(
			"align".to_string(),
			Property::new("Cell text alignment".to_string(), None),
		);

		// Row properties
		cell_props.insert(
			"ralign".to_string(),
			Property::new("Row text alignment".to_string(), None),
		);
		cell_props.insert(
			"rvspan".to_string(),
			Property::new("Row vertical span".to_string(), None),
		);

		// Column properties
		cell_props.insert(
			"chspan".to_string(),
			Property::new("Column horizontal span".to_string(), None),
		);

		// Table properties
		cell_props.insert(
			"talign".to_string(),
			Property::new("Table text alignment".to_string(), None),
		);

		Self {
			properties: PropertyParser { properties: props },
			cell_properties: PropertyParser {
				properties: cell_props,
			},
			re: Regex::new(
				r"(?:(?:^|\n):TABLE(?:\[((?:\\.|[^\\\\])*?)\])?(?:[^\S\r\n]+?\{(.*)\})?(.*))?(?:^|\n)(\|)",
			)
			.unwrap(),
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
		_match_data: Box<dyn Any>,
	) -> (Cursor, Vec<Report>) {
		let mut reports = vec![];
		let mut end_cursor = cursor.clone();
		let Some(table_capture) = self.re.captures_at(cursor.source.content(), cursor.pos) else {
			panic!("Invalid table regex");
		};
		if cursor.source.content().as_bytes()[cursor.pos] == b'\n' {
			end_cursor.pos += 1;
		}

		// Semantics for the speficier
		if cursor.source.content().as_bytes()[end_cursor.pos] == b':' {
			if let Some((sems, tokens)) =
				Semantics::from_source(cursor.source.clone(), &state.shared.lsp)
			{
				if let Some(range) = table_capture.get(0).map(|m| m.range()) {
					let start = if cursor.source.content().as_bytes()[range.start] == b'\n' {
						range.start + 1
					} else {
						range.start
					};
					sems.add(start..start + ":TABLE".len(), tokens.table_specifier);
				}
			}
		}

		// Parse properties
		let prop_source = escape_source(
			cursor.source.clone(),
			table_capture.get(1).map_or(0..0, |m| m.range()),
			"Table Properties".into(),
			'\\',
			"]",
		);
		let properties = match self.properties.parse(
			"Table",
			&mut reports,
			state,
			Token::new(0..prop_source.content().len(), prop_source),
		) {
			Some(props) => props,
			None => return (end_cursor, reports),
		};

		// Properties semantics
		if let (Some((sems, tokens)), Some(props)) = (
			Semantics::from_source(cursor.source.clone(), &state.shared.lsp),
			table_capture.get(1).map(|m| m.range()),
		) {
			sems.add(props.start - 1..props.start, tokens.table_props_sep);
			sems.add(props.end..props.end + 1, tokens.table_props_sep);
		}

		let export_as = match properties.get_opt(&mut reports, "export_as", |_, value| {
			Result::<_, String>::Ok(value.value.clone())
		}) {
			Some(name) => name,
			None => return (end_cursor, reports),
		};

		// Get table refname if any
		let refname = match table_capture.get(2) {
			Some(m) => match validate_refname(document, m.as_str(), true) {
				Ok(name) => {
					// Reference semantics
					if let Some((sems, tokens)) =
						Semantics::from_source(cursor.source.clone(), &state.shared.lsp)
					{
						sems.add(m.start() - 1..m.end() + 1, tokens.table_reference);
					}
					Some(name.to_owned())
				}
				Err(err) => {
					report_err!(
						&mut reports,
						cursor.source.clone(),
						"Invalid Table Refname".into(),
						span(
							m.range(),
							format!(
								"Reference name `{}` is invalid for a table: {err}",
								m.as_str().fg(state.parser.colors().highlight),
							)
						),
					);
					return (end_cursor, reports);
				}
			},
			None => None,
		};

		// Get table title if any
		let title = match table_capture.get(3) {
			Some(m) => {
				let title = m.as_str().trim();
				if !title.is_empty() {
					// Title semantics
					if let Some((sems, tokens)) =
						Semantics::from_source(cursor.source.clone(), &state.shared.lsp)
					{
						sems.add(m.start()..m.end(), tokens.table_title);
					}

					Some(title.to_owned())
				} else {
					None
				}
			}
			None => None,
		};

		end_cursor.pos = table_capture.get(4).unwrap().start();

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

			// Insert column and rows
			while table_state.rows.len() <= cell_pos.1 {
				table_state.rows.push(None)
			}
			while table_state.columns.len() <= cell_pos.0 {
				table_state.columns.push(None)
			}

			// Get properties
			let prop_source = escape_source(
				cursor.source.clone(),
				captures.get(1).map_or(0..0, |m| m.range()),
				"Cell Properties".into(),
				'\\',
				":",
			);
			let properties = match self.cell_properties.parse(
				"Table",
				&mut reports,
				state,
				prop_source.clone().into(),
			) {
				Some(props) => props,
				None => return (end_cursor, reports),
			};
			let cell_properties = match parse_properties(
				&properties,
				&mut reports,
				state,
				&cell_pos,
				&mut table_state,
			) {
				Some(props) => props,
				None => return (end_cursor, reports),
			};
			let hspan = cell_properties.hspan.unwrap_or(1);
			let vspan = cell_properties.vspan.unwrap_or(1);

			// Semantics
			if let Some((sems, tokens)) =
				Semantics::from_source(cursor.source.clone(), &state.shared.lsp)
			{
				sems.add(range.start..range.start + 1, tokens.table_sep);
				if let Some(range) = captures.get(1).map(|m| m.range()) {
					sems.add(range.start - 1..range.start, tokens.table_props_sep);
					sems.add(range.end..range.end + 1, tokens.table_props_sep);
				}
			}

			// Parse cell
			let cell_source = escape_source(
				end_cursor.source.clone(),
				captures.get(2).unwrap().range(),
				format!(":Cell:({}, {})", cell_pos.0, cell_pos.1),
				'\\',
				"|",
			);
			// Check for overlaps in case the cell is not empty
			if !prop_source.content().is_empty() || cell_source.content().trim_start().len() != 0 {
				if let Some((overlap_range, pos, size)) = table_state
					.overlaps
					.is_occupied(&cell_pos, &GridPosition(hspan, vspan))
				{
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

			// If empty, insert reference to owning cell, may insert multiple references
			if prop_source.content().is_empty() && cell_source.content().trim_start().len() == 0 {
				if let Some((_overlap_range, pos, size)) = table_state
					.overlaps
					.is_occupied(&cell_pos, &GridPosition(1, 1))
				{
					for _i in 0..size.0 {
						cells.push(Cell::Reference(pos.0 + pos.1 * dimensions.1));
					}
					cell_pos.0 += size.0 - 1;
				} else {
					// No overlap, insert empty cell as owning
					cells.push(Cell::Owning(CellData {
						location: Token::new(
							captures.get(2).unwrap().range(),
							end_cursor.source.clone(),
						),
						content_location: cell_source.into(),
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
					content_location: cell_source.into(),
					content: parsed_content,
					properties: cell_properties.clone(),
				}));

				for _i in 0..hspan - 1 {
					cells.push(Cell::Reference(cell_pos.0 + cell_pos.1 * dimensions.1));
				}
			}

			// Insert cell to the overlaps if not a (1, 1) cell
			if vspan != 1 || hspan != 1 {
				table_state.overlaps.push((
					range.start + 1..range.end - 1,
					GridPosition(cell_pos.0, cell_pos.1),
					GridPosition(cell_pos.0 + hspan, cell_pos.1 + vspan),
				));
			}

			// Update width
			cell_pos.0 += hspan;
			if table_state.first_row {
				dimensions.0 += hspan;
			}

			end_cursor.pos = range.end - 1;
			if end_cursor.source.content().as_bytes()[end_cursor.pos + 1] != b'\n' {
				// Next column
				if cell_pos.0 > dimensions.0 {
					dimensions.0 = cell_pos.0;
				}

				table_state.current_row = table_state.current_row.start..end_cursor.pos;
			} else {
				if let Some((sems, tokens)) =
					Semantics::from_source(cursor.source.clone(), &state.shared.lsp)
				{
					sems.add(range.end - 1..range.end, tokens.table_sep);
				}

				// Next row
				table_state.current_row = table_state.current_row.start..end_cursor.pos + 1;
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

		// Go back 1 character
		if cursor.source.content().as_bytes()[end_cursor.pos - 1] == b'\n' {
			end_cursor.pos -= 1;
		}

		// Checks for cells whose height leads outside the table
		if let Some((overlap_range, _, size)) = table_state
			.overlaps
			.is_occupied(&GridPosition(0, cell_pos.1), &GridPosition(1, dimensions.1))
		{
			report_err!(
				&mut reports,
				cursor.source.clone(),
				"Invalid Table Cell".into(),
				span(
					overlap_range,
					format!(
						"Cell {} has height {}, which is outside the table",
						cell_pos.fg(state.parser.colors().info),
						size.1.fg(state.parser.colors().info)
					)
				)
			);
			return (end_cursor, reports);
		}

		if let Some(export_as) = export_as {
			let mut kernels_borrow = state.shared.kernels.borrow_mut();
			let kernel = kernels_borrow.get("main").unwrap();

			let mut columns = Vec::with_capacity(dimensions.1);
			for i in 0..dimensions.1 {
				let mut row = Vec::with_capacity(dimensions.0);
				for j in 0..dimensions.0 {
					match &cells[j + i * dimensions.0] {
						Cell::Owning(cell_data) => row.push(
							cell_data.content_location.source().content()
								[cell_data.content_location.range.clone()]
							.to_owned(),
						),
						Cell::Reference(id) => {
							if let Cell::Owning(cell_data) = &cells[*id] {
								row.push(
									cell_data.content_location.source().content()
										[cell_data.content_location.range.clone()]
									.to_owned(),
								)
							}
						}
					}
				}

				columns.push(row);
			}

			if let Err(err) = kernel.export_table(export_as.as_str(), columns) {
				report_err!(
					&mut reports,
					cursor.source.clone(),
					"Failed to export lua table".into(),
					span(
						table_capture.get(0).unwrap().range(),
						format!(
							"Table `{}` could not be exported: {err}",
							export_as.fg(state.parser.colors().info)
						)
					)
				);
			}
		}

		state.push(
			document,
			Box::new(Table {
				location: Token::new(cursor.pos..end_cursor.pos, cursor.source.clone()),
				size: (dimensions.0, dimensions.1),
				columns: table_state.columns,
				rows: table_state.rows,
				properties: table_state.properties,
				data: cells,
				reference: refname,
				title,
			}),
		);

		(end_cursor, reports)
	}
}
