use serde::Serialize;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::CompilerOutput;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::reports::Report;
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

	fn compile<'e>(
		&self,
		compiler: &Compiler,
		_document: &dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => match (self.kind, self.numbered) {
				(MarkerKind::Close, true) => output.add_content("</ol>"),
				(MarkerKind::Close, false) => output.add_content("</ul>"),
				(MarkerKind::Open, true) => output.add_content("<ol>"),
				(MarkerKind::Open, false) => output.add_content("<ul>"),
			},
			_ => todo!(),
		}
		Ok(())
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

	fn compile<'e>(
		&'e self,
		compiler: &'e Compiler,
		document: &'e dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				if let Some((numbered, number)) = self.numbering.last() {
					if *numbered {
						output.add_content(format!("<li value=\"{number}\">"));
					} else {
						output.add_content("<li>");
					}
				}
				if let Some(CustomListData::Checkbox(checkbox_state)) = &self.custom { match checkbox_state {
    						CheckboxState::Unchecked => {
    							output.add_content(
    								r#"<input type="checkbox" class="checkbox-unchecked" onclick="return false;">"#,
    							);
    						}
    						CheckboxState::Partial => {
    							output.add_content(
    								r#"<input type="checkbox" class="checkbox-partial" onclick="return false;">"#,
    							);
    						}
    						CheckboxState::Checked => {
    							output.add_content(
    								r#"<input type="checkbox" class="checkbox-checked" onclick="return false;" checked>"#,
    							);
    						}
    					} }
				for elem in &self.content {
					elem.compile(compiler, document, output)?;
				}
				output.add_content("</li>");
			}
			_ => todo!(),
		}
		Ok(())
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
