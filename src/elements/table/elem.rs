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
	// TODO: Unspecified constraints should lookup to the column
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
		todo!()
	}
}
