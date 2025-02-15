use std::cell::RefCell;
use std::rc::Rc;
use std::str::FromStr;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::scope::Scope;
use crate::parser::source::Token;
use downcast_rs::impl_downcast;
use downcast_rs::Downcast;
use url::Url;


/// The kind for an element
///
/// The kind of an element determines how it affects paragraphing as well as nested elements.
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
	fn compile(
		&self,
		scope: Rc<RefCell<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>>;
}
impl_downcast!(Element);

pub trait ReferenceableElement: Element {
	/// Reference name
	fn reference_name(&self) -> Option<&String>;

	/// Key for refcounting
	///
	/// Each unique key will have a unique associated counter.
	/// This is used to have different counters when referencing tables, sections or media.
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

	/// Gets an url to the reference, based on the compiler's target
	//fn get_url(&self, target: Target) -> Url;
}

pub trait ContainerElement: Element {
	/// Gets the contained elements
	fn contained(&self) -> &[Rc<RefCell<Scope>>];
}

#[derive(Debug)]
pub struct DocumentEnd(pub Token);

impl Element for DocumentEnd {
	fn location(&self) -> &Token { &self.0 }

	fn kind(&self) -> ElemKind { ElemKind::Invisible }

	fn element_name(&self) -> &'static str { "Document End" }

	fn compile<'e>(
		&'e self,
		_scope: Rc<RefCell<Scope>>,
		_compiler: &'e Compiler,
		_output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		Ok(())
	}
}
