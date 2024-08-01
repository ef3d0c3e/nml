use std::cell::Ref;
use std::cell::RefCell;
use std::cell::RefMut;
use std::collections::hash_map::HashMap;
use std::rc::Rc;

use crate::parser::source::Source;

use super::element::Element;
use super::element::ReferenceableElement;
use super::variable::Variable;

#[derive(Debug, Clone, Copy)]
pub enum ElemReference {
	Direct(usize),

	// Reference nested inside another element, e.g [`Paragraph`] or [`Media`]
	Nested(usize, usize),
}

#[derive(Debug)]
pub struct Scope {
	/// List of all referenceable elements in current scope.
	/// All elements in this should return a non empty
	pub referenceable: HashMap<String, ElemReference>,
	pub variables: HashMap<String, Rc<dyn Variable>>,
}

impl Scope {
	pub fn new() -> Self {
		Self {
			referenceable: HashMap::new(),
			variables: HashMap::new(),
		}
	}

	pub fn merge(&mut self, other: &mut Scope, merge_as: &String, ref_offset: usize) {
		match merge_as.is_empty() {
			true => {
				// References
				self.referenceable.extend(other.referenceable.drain().map(
					|(name, idx)| match idx {
						ElemReference::Direct(index) => {
							(name, ElemReference::Direct(index + ref_offset))
						}
						ElemReference::Nested(index, sub_index) => {
							(name, ElemReference::Nested(index + ref_offset, sub_index))
						}
					},
				));

				// Variables
				self.variables
					.extend(other.variables.drain().map(|(name, var)| (name, var)));
			}
			false => {
				// References
				self.referenceable.extend(other.referenceable.drain().map(
					|(name, idx)| match idx {
						ElemReference::Direct(index) => (
							format!("{merge_as}.{name}"),
							ElemReference::Direct(index + ref_offset),
						),
						ElemReference::Nested(index, sub_index) => (
							format!("{merge_as}.{name}"),
							ElemReference::Nested(index + ref_offset, sub_index),
						),
					},
				));

				// Variables
				self.variables.extend(
					other
						.variables
						.drain()
						.map(|(name, var)| (format!("{merge_as}.{name}"), var)),
				);
			}
		}
	}
}

pub trait Document<'a>: core::fmt::Debug {
	/// Gets the document [`Source`]
	fn source(&self) -> Rc<dyn Source>;

	/// Gets the document parent (if it exists)
	fn parent(&self) -> Option<&'a dyn Document<'a>>;

	/// Gets the document content
	/// The content is essentially the AST for the document
	fn content(&self) -> &RefCell<Vec<Box<dyn Element>>>;

	/// Gets the document [`Scope`]
	fn scope(&self) -> &RefCell<Scope>;

	/// Pushes a new element into the document's content
	fn push(&self, elem: Box<dyn Element>) {
		if let Some(refname) = elem
			.as_referenceable()
			.and_then(|reference| reference.reference_name())
		{
			self.scope().borrow_mut().referenceable.insert(
				refname.clone(),
				ElemReference::Direct(self.content().borrow().len()),
			);
		} else if let Some(container) = self
			.content()
			.borrow()
			.last()
			.and_then(|elem| elem.as_container())
		{
			// This is a hack that works thanks to the fact that at document end, a [`DocumentEnd`] elem is pushed
			container
				.contained()
				.iter()
				.enumerate()
				.for_each(|(sub_idx, elem)| {
					if let Some(refname) = elem
						.as_referenceable()
						.and_then(|reference| reference.reference_name())
					{
						self.scope().borrow_mut().referenceable.insert(
							refname.clone(),
							ElemReference::Nested(self.content().borrow().len() - 1, sub_idx),
						);
					}
				});
		}

		self.content().borrow_mut().push(elem);
	}

	fn add_variable(&self, variable: Rc<dyn Variable>) {
		self.scope()
			.borrow_mut()
			.variables
			.insert(variable.name().to_string(), variable);
	}

	fn get_variable(&self, name: &str) -> Option<Rc<dyn Variable>> {
		match self.scope().borrow().variables.get(name) {
			Some(variable) => {
				return Some(variable.clone());
			}

			// Continue search recursively
			None => match self.parent() {
				Some(parent) => return parent.get_variable(name),

				// Not found
				None => return None,
			},
		}
	}

	/// Merges [`other`] into [`self`]
	///
	/// # Parameters
	///
	/// If [`merge_as`] is None, references and variables from the other document are not merged into self
	fn merge(
		&self,
		content: &RefCell<Vec<Box<dyn Element>>>,
		scope: &RefCell<Scope>,
		merge_as: Option<&String>,
	) {
		match merge_as {
			Some(merge_as) => self.scope().borrow_mut().merge(
				&mut *scope.borrow_mut(),
				merge_as,
				self.content().borrow().len() + 1,
			),
			_ => {}
		}

		// Content
		self.content()
			.borrow_mut()
			.extend((content.borrow_mut()).drain(..).map(|value| value));
	}

	fn get_reference(&self, refname: &str) -> Option<ElemReference> {
		self.scope()
			.borrow()
			.referenceable
			.get(refname)
			.and_then(|reference| Some(*reference))
	}

	fn get_from_reference(
		&self,
		reference: &ElemReference,
	) -> Option<Ref<'_, dyn ReferenceableElement>> {
		match reference {
			ElemReference::Direct(idx) => Ref::filter_map(self.content().borrow(), |content| {
				content.get(*idx).and_then(|elem| elem.as_referenceable())
			})
			.ok(),
			ElemReference::Nested(idx, sub_idx) => {
				Ref::filter_map(self.content().borrow(), |content| {
					content
						.get(*idx)
						.and_then(|elem| elem.as_container())
						.and_then(|container| container.contained().get(*sub_idx))
						.and_then(|elem| elem.as_referenceable())
				})
				.ok()
			}
		}
	}
}

pub trait DocumentAccessors<'a> {
	fn last_element<T: Element>(&self) -> Option<Ref<'_, T>>;
	fn last_element_mut<T: Element>(&self) -> Option<RefMut<'_, T>>;
}

impl<'a> DocumentAccessors<'a> for dyn Document<'a> + '_ {
	fn last_element<T: Element>(&self) -> Option<Ref<'_, T>> {
		Ref::filter_map(self.content().borrow(), |content| {
			content.last().and_then(|last| last.downcast_ref::<T>())
		})
		.ok()
	}

	fn last_element_mut<T: Element>(&self) -> Option<RefMut<'_, T>> {
		RefMut::filter_map(self.content().borrow_mut(), |content| {
			content.last_mut().and_then(|last| last.downcast_mut::<T>())
		})
		.ok()
	}
}


#[cfg(test)]
pub mod tests
{
#[macro_export]
macro_rules! validate_document {
	($container:expr, $idx:expr,) => {};
	($container:expr, $idx:expr, $t:ty; $($tail:tt)*) => {{
		let elem = &$container[$idx];
		assert!(elem.downcast_ref::<$t>().is_some(), "Invalid element at index {}, expected {}", $idx, stringify!($t));

		validate_document!($container, ($idx+1), $($tail)*);
	}};
	($container:expr, $idx:expr, $t:ty { $($field:ident == $value:expr),* }; $($tail:tt)*) => {{
		let elem = &$container[$idx];
		assert!(elem.downcast_ref::<$t>().is_some(), "Invalid element at index {}, expected {}", $idx, stringify!($t));

		$(
			let val = &elem.downcast_ref::<$t>().unwrap().$field;
			assert!(*val == $value, "Invalid field {} for {} at index {}, expected {:#?}, found {:#?}",
				stringify!($field),
				stringify!($t),
				$idx,
				$value,
				val);
		)*

			validate_document!($container, ($idx+1), $($tail)*);
	}};
	($container:expr, $idx:expr, $t:ty { $($ts:tt)* }; $($tail:tt)*) => {{
		let elem = &$container[$idx];
		assert!(elem.downcast_ref::<$t>().is_some(), "Invalid container element at index {}, expected {}", $idx, stringify!($t));

		let contained = elem.as_container().unwrap().contained();
		validate_document!(contained, 0, $($ts)*);

		validate_document!($container, ($idx+1), $($tail)*);
	}};
}
}
