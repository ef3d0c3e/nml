use serde::Serialize;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::source::Token;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum MarkerKind {
	Open,
	Close,
}

#[derive(Debug)]
pub struct ListMarker {
	pub(crate) location: Token,
	pub(crate) numbered: bool,
	pub(crate) kind: MarkerKind,
}

impl Element for ListMarker {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Block }

	fn element_name(&self) -> &'static str { "List Marker" }

	fn compile(
		&self,
		compiler: &Compiler,
		_document: &dyn Document,
		_cursor: usize,
	) -> Result<String, String> {
		match compiler.target() {
			HTML => match (self.kind, self.numbered) {
				(MarkerKind::Close, true) => Ok("</ol>".to_string()),
				(MarkerKind::Close, false) => Ok("</ul>".to_string()),
				(MarkerKind::Open, true) => Ok("<ol>".to_string()),
				(MarkerKind::Open, false) => Ok("<ul>".to_string()),
			},
			_ => todo!(),
		}
	}
}

/// State of a checkbox
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum CheckboxState {
	Unchecked,
	Partial,
	Checked,
}

/// Customization data for the list
#[derive(Debug, PartialEq, Eq)]
pub enum CustomListData {
	Checkbox(CheckboxState),
}

#[derive(Debug)]
pub struct ListEntry {
	pub(crate) location: Token,
	pub(crate) numbering: Vec<(bool, usize)>,
	pub(crate) content: Vec<Box<dyn Element>>,
	pub(crate) bullet: Option<String>,
	pub(crate) custom: Option<CustomListData>,
}

impl Element for ListEntry {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Block }

	fn element_name(&self) -> &'static str { "List Entry" }

	fn compile(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		cursor: usize,
	) -> Result<String, String> {
		match compiler.target() {
			HTML => {
				let mut result = String::new();
				if let Some((numbered, number)) = self.numbering.last() {
					if *numbered {
						result += format!("<li value=\"{number}\">").as_str();
					} else {
						result += "<li>";
					}
				}
				match &self.custom {
					Some(CustomListData::Checkbox(checkbox_state)) => match checkbox_state {
						CheckboxState::Unchecked => {
							result += r#"<input type="checkbox" class="checkbox-unchecked" onclick="return false;">"#
						}
						CheckboxState::Partial => {
							result += r#"<input type="checkbox" class="checkbox-partial" onclick="return false;">"#
						}
						CheckboxState::Checked => {
							result += r#"<input type="checkbox" class="checkbox-checked" onclick="return false;" checked>"#
						}
					},
					_ => {}
				}
				for elem in &self.content {
					result += elem
						.compile(compiler, document, cursor + result.len())?
						.as_str();
				}
				result += "</li>";
				Ok(result)
			}
			_ => todo!(),
		}
	}

	fn as_container(&self) -> Option<&dyn ContainerElement> { Some(self) }
}

impl ContainerElement for ListEntry {
	fn contained(&self) -> &Vec<Box<dyn Element>> { &self.content }

	fn push(&mut self, elem: Box<dyn Element>) -> Result<(), String> {
		if elem.kind() == ElemKind::Block {
			return Err("Cannot add block element inside a list".to_string());
		}

		self.content.push(elem);
		Ok(())
	}
}
