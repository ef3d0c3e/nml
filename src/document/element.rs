use std::str::FromStr;

use crate::compiler::compiler::Compiler;
use crate::parser::source::Token;
use downcast_rs::impl_downcast;
use downcast_rs::Downcast;

use super::document::Document;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ElemKind {
	/// An invisible element (i.e comment)
	Invisible,
	/// Special elements don't trigger special formatting events
	Special,
	/// Inline elements don't break paragraphing
	Inline,
	/// Block elements are outside of paragraphs
	Block,
}

impl FromStr for ElemKind {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"invisible" => Ok(ElemKind::Invisible),
			"special" => Ok(ElemKind::Special),
			"inline" => Ok(ElemKind::Inline),
			"block" => Ok(ElemKind::Block),
			_ => Err(format!("Unknown ElemKind: {s}")),
		}
	}
}

pub trait Element: Downcast {
	/// Gets the element defined location i.e token without filename
	fn location(&self) -> &Token;

	fn kind(&self) -> ElemKind;

	/// Get the element's name
	fn element_name(&self) -> &'static str;

	/// Outputs element to string for debug purposes
	fn to_string(&self) -> String;

	/// Gets the element as a referenceable i.e an element that can be referenced
	fn as_referenceable(&self) -> Option<&dyn ReferenceableElement> { None }

	/// Gets the element as a container containing other elements
	fn as_container(&self) -> Option<&dyn ContainerElement> { None }

	/// Compiles element
	fn compile(&self, compiler: &Compiler, document: &dyn Document) -> Result<String, String>;
}
impl_downcast!(Element);

impl core::fmt::Debug for dyn Element {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.to_string())
	}
}

pub trait ReferenceableElement: Element {
	/// Reference name
	fn reference_name(&self) -> Option<&String>;

	/// Key for refcounting
	fn refcount_key(&self) -> &'static str;
}

impl core::fmt::Debug for dyn ReferenceableElement {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.to_string())
	}
}

pub trait ContainerElement: Element {
	/// Gets the contained elements
	fn contained(&self) -> &Vec<Box<dyn Element>>;

	/// Adds an element to the container
	fn push(&mut self, elem: Box<dyn Element>) -> Result<(), String>;
}

impl core::fmt::Debug for dyn ContainerElement {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.to_string())
	}
}

#[derive(Debug)]
pub struct DocumentEnd(pub Token);

impl Element for DocumentEnd {
	fn location(&self) -> &Token { &self.0 }

	fn kind(&self) -> ElemKind { ElemKind::Invisible }

	fn element_name(&self) -> &'static str { "Document End" }

	fn to_string(&self) -> String { format!("{self:#?}") }

	fn compile(&self, _compiler: &Compiler, _document: &dyn Document) -> Result<String, String> {
		Ok(String::new())
	}
}
