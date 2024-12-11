use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::source::Token;

/// Formatting for cells
#[derive(Debug, Clone)]
pub struct CellProperties {
	/// Vertial span of the cell
	pub(crate) vspan: usize,
	/// Horizontal span of the cell
	pub(crate) hspan: usize,
	// TODO: Contains borders as well as their display
	// TODO: Unspecified constraints should lookup to the column/row
}

/// The data inside of a table's cell
#[derive(Debug)]
pub struct CellData {
	pub(crate) location: Token,
	pub(crate) content: Vec<Box<dyn Element>>,
	pub(crate) properties: CellProperties,
}

/// Represents cells inside a table, with possible references for fused cells
#[derive(Debug)]
pub enum Cell {
	Owning(CellData),
	/// A reference simply holds the index of the owning cell in the table's buffer
	Reference(usize),
}

/// Data for columns
#[derive(Debug, Default)]
pub struct ColumnData {
	// TODO: Contains alignment, formatting e.g vertical lines & style
}

/// Data for rows
#[derive(Debug, Default)]
pub struct RowData {
	// TODO: formatting e.g horizontal lines & style
}

/// The table
#[derive(Debug)]
pub struct Table {
	pub(crate) location: Token,
	/// Number of colunms and rows in the table
	pub(crate) size: (usize, usize),
	/// Data for each column
	pub(crate) columns: Vec<Option<ColumnData>>,
	/// Data for each row
	pub(crate) rows: Vec<Option<RowData>>,
	/// Content of the table
	pub(crate) data: Vec<Cell>,
}

impl Element for Table {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Block }

	fn element_name(&self) -> &'static str { "Table" }

	fn compile(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		cursor: usize,
	) -> Result<String, String> {
		// TODO: colgroup
		let mut result = String::new();

		let mut pos = (0usize, 0usize);
		result += "<table>";
		for cell in &self.data {
			if pos.0 == 0 {
				result += "<tr>";
			}
			match cell {
				Cell::Owning(cell_data) => {
					// TODO: Rowgroup
					match (cell_data.properties.hspan, cell_data.properties.vspan) {
						(1, 1) => result += "<td>",
						(1, v) => result += format!("<td rowspan=\"{v}\">").as_str(),
						(h, 1) => result += format!("<td colspan=\"{h}\">").as_str(),
						(h, v) => {
							result += format!("<td rowspan=\"{v}\" colspan=\"{h}\">").as_str()
						}
					}
					for elem in &cell_data.content {
						result += elem
							.compile(compiler, document, cursor + result.len())?
							.as_str();
					}
					result += "</td>";
				}
				Cell::Reference(id) => {
					if let Cell::Owning(cell_data) = &self.data[*id] {
						pos.0 += cell_data.properties.hspan - 1;
					} else {
						panic!("Invalid cells");
					}
				}
			}

			// Advance position
			pos.0 += 1;
			if pos.0 == self.size.0 {
				result += "</tr>";
				pos.0 = 0;
				pos.1 += 1;
			}
		}
		result += "</table>";

		Ok(result)
	}
}
