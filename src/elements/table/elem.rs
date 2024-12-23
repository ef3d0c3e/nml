use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::source::Token;

/// Converts to style
trait ToStyle {
	fn to_style(&self, target: Target) -> String;
}

/// Text alignment for table cells
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
	#[default]
	Left,
	Right,
	Center,
}

impl TryFrom<&String> for Align {
	type Error = String;

	fn try_from(value: &String) -> Result<Self, Self::Error> {
		match value.as_str() {
			"left" => Ok(Align::Left),
			"right" => Ok(Align::Right),
			"center" => Ok(Align::Center),
			_ => Err(format!("Unknown alignment: `{value}`")),
		}
	}
}

impl ToStyle for Option<Align> {
	fn to_style(&self, target: Target) -> String {
		match target {
			HTML => match self {
				Some(Align::Right) => "text-align: right;".into(),
				Some(Align::Center) => "text-align: center;".into(),
				Some(Align::Left) | None => "".into(),
			},
			_ => todo!(),
		}
	}
}

/// Border style for cells
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStyle {
	#[default]
	Solid,
	Dashed,
	Dotted,
	None,
}

impl ToStyle for Option<BorderStyle> {
	fn to_style(&self, target: Target) -> String {
		if self.is_none() {
			return String::new();
		}
		todo!();
	}
}

impl TryFrom<&String> for BorderStyle {
	type Error = String;

	fn try_from(value: &String) -> Result<Self, Self::Error> {
		match value.as_str() {
			"solid" => Ok(BorderStyle::Solid),
			"dashed" => Ok(BorderStyle::Dashed),
			"dotted" => Ok(BorderStyle::Dotted),
			"none" => Ok(BorderStyle::None),
			_ => Err(format!("Unknown border style: `{value}`")),
		}
	}
}

/// Formatting for cells
#[derive(Debug, Clone)]
pub struct CellProperties {
	/// Vertial span of the cell
	pub(crate) vspan: Option<usize>,
	/// Horizontal span of the cell
	pub(crate) hspan: Option<usize>,
	/// Text alignment for the cell
	pub(crate) align: Option<Align>,
	/// Borders formatting for the cell
	pub(crate) borders: [Option<BorderStyle>; 4],
}

impl ToStyle for CellProperties {
	fn to_style(&self, target: Target) -> String {
		let mut style = String::new();
		style += self.align.to_style(target).as_str();
		for border in &self.borders {
			style += border.to_style(target).as_str();
		}
		return style;
	}
}

/// Data for columns
#[derive(Debug)]
pub struct ColumnProperties {
	/// Span for the cells in this column
	pub(crate) hspan: Option<usize>,
	/// Borders formatting for cells in this column
	pub(crate) borders: [Option<BorderStyle>; 4],
}

impl ToStyle for Option<ColumnProperties> {
	fn to_style(&self, target: Target) -> String {
		if self.is_none() {
			return String::new();
		}

		let props = self.as_ref().unwrap();
		let mut style = String::new();
		for border in &props.borders {
			style += border.to_style(target).as_str();
		}
		return style;
	}
}

/// Data for rows
#[derive(Debug)]
pub struct RowProperties {
	/// Span for the cells in this row
	pub(crate) vspan: Option<usize>,
	/// Text alignment for the cells in this row
	pub(crate) align: Option<Align>,
	/// Borders formatting for cells in this row
	pub(crate) borders: [Option<BorderStyle>; 4],
}

impl ToStyle for Option<RowProperties> {
	fn to_style(&self, target: Target) -> String {
		if self.is_none() {
			return String::new();
		}

		let props = self.as_ref().unwrap();
		let mut style = String::new();
		style += props.align.to_style(target).as_str();
		for border in &props.borders {
			style += border.to_style(target).as_str();
		}
		return style;
	}
}

/// Data for entire table
#[derive(Default, Debug)]
pub struct TableProperties {
	/// Text alignment for the cells in this table
	pub(crate) align: Option<Align>,
	/// Borders formatting for cells in this table
	pub(crate) borders: [Option<BorderStyle>; 4],
}

impl ToStyle for TableProperties {
	fn to_style(&self, target: Target) -> String {
		let mut style = String::new();
		style += self.align.to_style(target).as_str();
		for border in &self.borders {
			style += border.to_style(target).as_str();
		}
		return style;
	}
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

/// The table
#[derive(Debug)]
pub struct Table {
	/// Token for the table
	pub(crate) location: Token,
	/// Number of colunms and rows in the table
	pub(crate) size: (usize, usize),
	/// Properties for each columns
	pub(crate) columns: Vec<Option<ColumnProperties>>,
	/// Properties for each rows
	pub(crate) rows: Vec<Option<RowProperties>>,
	/// Properties for the entire table
	pub(crate) properties: TableProperties,
	/// Content of the table
	pub(crate) data: Vec<Cell>,
}

impl Element for Table {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Block
	}

	fn element_name(&self) -> &'static str {
		"Table"
	}

	fn compile(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		cursor: usize,
	) -> Result<String, String> {
		// TODO: colgroup
		let mut result = String::new();

		let table_style = self.properties.to_style(compiler.target());

		// Colgroup styling
		let colgroup = if self.columns.iter().fold(false, |v, col| v || col.is_some()) {
			let mut result = "<colgroup>".to_string();
			for col in &self.columns {
				let style = col.to_style(compiler.target());
				result += "<col";
				if let Some(span) = col.as_ref().and_then(|c| c.hspan) {
					result += format!(" span=\"{span}\"").as_str();
				}
				if !style.is_empty() {
					result += " style=\"";
					result += style.as_str();
					result += "\"";
				}
				result += ">";
			}
			result + "</colgroup>"
		} else {
			String::new()
		};

		let mut pos = (0usize, 0usize);
		if !table_style.is_empty() {
			result += format!("<table style=\"{table_style}\">").as_str()
		} else {
			result += "<table>"
		}
		result += colgroup.as_str();
		for cell in &self.data {
			if pos.0 == 0 {
				// Row styling
				let style = {
					let result = self.rows[pos.1].to_style(compiler.target());
					if result.is_empty() {
						result
					} else {
						format!(" style=\"{result}\"")
					}
				};

				if let Some(span) = self.rows[pos.1].as_ref().and_then(|row| row.vspan) {
					result += format!("<tr span=\"{span}\"{style}>").as_str();
				} else {
					result += format!("<tr{style}>").as_str();
				}
			}
			match cell {
				Cell::Owning(cell_data) => {
					// Cell styling
					let style = {
						let result = cell_data.properties.to_style(compiler.target());
						if result.is_empty() {
							result
						} else {
							format!(" style=\"{result}\"")
						}
					};

					let (hspan, vspan) = (
						cell_data.properties.hspan.unwrap_or(1),
						cell_data.properties.vspan.unwrap_or(1),
					);

					match (hspan, vspan) {
						(1, 1) => result += format!("<td{style}>").as_str(),
						(1, v) => result += format!("<td rowspan=\"{v}\"{style}>").as_str(),
						(h, 1) => result += format!("<td colspan=\"{h}\"{style}>").as_str(),
						(h, v) => {
							result +=
								format!("<td rowspan=\"{v}\" colspan=\"{h}\"{style}>").as_str()
						}
					}
					for elem in &cell_data.content {
						result += elem
							.compile(compiler, document, cursor + result.len())?
							.as_str();
					}
					result += "</td>";
				}
				Cell::Reference(_) => {}
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
