use std::cell::RefCell;
use std::rc::Rc;
use std::str::FromStr;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::SourcePosition;
use crate::parser::source::Token;
use downcast_rs::impl_downcast;
use downcast_rs::Downcast;

use super::references::InternalReference;
use super::references::Refname;
use super::scope::Scope;
use super::scope::ScopeAccessor;
use super::unit::Reference;

/// The kind for an element
///
/// The kind of an element determines how it affects paragraphing as well as nested elements.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ElemKind {
	/// An invisible element (e.g comment)
	Invisible,
	/// Made of multiple smaller elements which need to be taken into account
	Compound,
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
			"compound" => Ok(ElemKind::Compound),
			"inline" => Ok(ElemKind::Inline),
			"block" => Ok(ElemKind::Block),
			_ => Err(format!("Unknown ElemKind: {s}")),
		}
	}
}

pub trait Element: Downcast + core::fmt::Debug {
	/// Gets the element defined location i.e token without filename
	fn location(&self) -> &Token;

	/// Gets the original byte range in the unit's source file
	fn original_location(&self) -> Token {
		self.location()
			.source()
			.original_range(self.location().range.clone())
	}

	/// The basic element kind
	fn kind(&self) -> ElemKind;

	/// Get the element's name
	fn element_name(&self) -> &'static str;

	/// Compiles element
	fn compile(
		&self,
		scope: Rc<RefCell<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>>;

	/// Gets the element as a referenceable i.e an element that can be referenced
	fn as_referenceable(self: Rc<Self>) -> Option<Rc<dyn ReferenceableElement>>;

	/// Gets the element as a linkable element, i.e needs to be resolved to an appropriate reference
	fn as_linkable(self: Rc<Self>) -> Option<Rc<dyn LinkableElement>>;

	/// Gets the element as a container containing other elements
	fn as_container(self: Rc<Self>) -> Option<Rc<dyn ContainerElement>>;
}
impl_downcast!(Element);

/// An element from which a reference can be extracted
pub trait ReferenceableElement: Element {
	/// Returns the internal reference
	fn reference(&self) -> Rc<InternalReference>;

	/// Key for refcounting
	///
	/// Each unique key will have a unique associated counter.
	/// This is used to have different counters when referencing tables, sections or media.
	fn refcount_key(&self) -> &'static str;

	/// Gets the refid for a compiler. The refid is some key that can be used from an external
	/// document to reference this element.
	fn refid(&self, compiler: &Compiler, refid: usize) -> String;
}

/// An element which can be linked to a reference
pub trait LinkableElement: Element {
	/// Refname this element wants to link to
	fn wants_refname(&self) -> &Refname;
	/// Gets whether this element requires linking
	fn wants_link(&self) -> bool;
	/// Sets the link of this reference
	fn link(&self, reference: Reference);
}

/// An element containing at least one scope
pub trait ContainerElement: Element {
	/// Gets the contained elements
	fn contained(&self) -> &[Rc<RefCell<Scope>>];

	/// Determines the element kind made up by the content of this element
	/// This is only used when the kind of an element is [`ElemKind::Compound`]
	fn nested_kind(&self) -> ElemKind {
		if self.kind() != ElemKind::Compound {
			return self.kind();
		}

		for contained in self.contained() {
			for it in contained.content_iter(true) {
				match it.1.kind() {
					ElemKind::Block => return ElemKind::Block,
					ElemKind::Compound => {
						if let Some(container) = it.1.as_container() {
							if container.nested_kind() == ElemKind::Block {
								return ElemKind::Block;
							}
						}
					}
					_ => {}
				}
			}
		}
		ElemKind::Inline
	}
}

/// Gets the nested kind of an [`Rc<dyn Element>`] this will either call
/// [`Element::kind`] or (if the element is a container) [`ContainerElement::nested_kind`].
pub fn nested_kind(elem: Rc<dyn Element>) -> ElemKind {
	let Some(container) = elem.clone().as_container() else {
		return elem.kind();
	};

	container.nested_kind()
}
