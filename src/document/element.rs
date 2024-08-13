use std::str::FromStr;

use crate::compiler::compiler::Compiler;
use crate::elements::reference::InternalReference;
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

pub trait Element: Downcast + core::fmt::Debug {
	/// Gets the element defined location i.e token without filename
	fn location(&self) -> &Token;

	fn kind(&self) -> ElemKind;

	/// Get the element's name
	fn element_name(&self) -> &'static str;

	/// Gets the element as a referenceable i.e an element that can be referenced
	fn as_referenceable(&self) -> Option<&dyn ReferenceableElement> { None }

	/// Gets the element as a container containing other elements
	fn as_container(&self) -> Option<&dyn ContainerElement> { None }

	/// Compiles element
	fn compile(&self, compiler: &Compiler, document: &dyn Document, cursor: usize) -> Result<String, String>;
}
impl_downcast!(Element);

pub trait ReferenceableElement: Element {
	/// Reference name
	fn reference_name(&self) -> Option<&String>;

	/// Key for refcounting
	fn refcount_key(&self) -> &'static str;

	/// Creates the reference element
	fn compile_reference(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		reference: &InternalReference,
		refid: usize,
	) -> Result<String, String>;

	/// Gets the refid for a compiler. The refid is some key that can be used from an external
	/// document to reference this element.
	fn refid(&self, compiler: &Compiler, refid: usize) -> String;
}

pub trait ContainerElement: Element {
	/// Gets the contained elements
	fn contained(&self) -> &Vec<Box<dyn Element>>;

	/// Adds an element to the container
	fn push(&mut self, elem: Box<dyn Element>) -> Result<(), String>;
}

#[derive(Debug)]
pub struct DocumentEnd(pub Token);

impl Element for DocumentEnd {
	fn location(&self) -> &Token { &self.0 }

	fn kind(&self) -> ElemKind { ElemKind::Invisible }

	fn element_name(&self) -> &'static str { "Document End" }

	fn compile(&self, _compiler: &Compiler, _document: &dyn Document, _cursor: usize) -> Result<String, String> {
		Ok(String::new())
	}
}
