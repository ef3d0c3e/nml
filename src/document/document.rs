use std::cell::Ref;
use std::cell::RefCell;
use std::cell::RefMut;
use std::collections::hash_map::HashMap;
use std::rc::Rc;

use crate::parser::source::Source;

use super::element::Element;
use super::variable::Variable;

// TODO: Referenceable rework
// Usize based referencing is not an acceptable method
// if we want to support deltas for the lsp
#[derive(Debug)]
pub struct Scope {
	/// List of all referenceable elements in current scope.
	/// All elements in this should return a non empty
	pub referenceable: HashMap<String, usize>,
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
				self.referenceable.extend(
					other
						.referenceable
						.drain()
						.map(|(name, idx)| (name, idx + ref_offset)),
				);

				// Variables
				self.variables
					.extend(other.variables.drain().map(|(name, var)| (name, var)));
			}
			false => {
				// References
				self.referenceable.extend(
					other
						.referenceable
						.drain()
						.map(|(name, idx)| (format!("{merge_as}.{name}"), idx + ref_offset)),
				);

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
		// TODO: RefTable

		self.content().borrow_mut().push(elem);
	}

	/*
	fn last_element(&'a self, recurse: bool) -> Option<Ref<'_, dyn Element>>
	{
		let elem = Ref::filter_map(self.content().borrow(),
		|content| content.last()
			.and_then(|last| last.downcast_ref::<Element>())
		).ok();


		if elem.is_some() || !recurse { return elem }

		match self.parent()
		{
			None => None,
			Some(parent) => parent.last_element(true),
		}
	}

	fn last_element_mut(&'a self, recurse: bool) -> Option<RefMut<'_, dyn Element>>
	{
		let elem = RefMut::filter_map(self.content().borrow_mut(),
		|content| content.last_mut()).ok();

		if elem.is_some() || !recurse { return elem }

		match self.parent()
		{
			None => None,
			Some(parent) => parent.last_element_mut(true),
		}
	}
	*/

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

	/*
	fn remove_variable(&self, name: &str) -> Option<Rc<dyn Variable>>
	{
		match self.scope().borrow_mut().variables.remove(name)
		{
			Some(variable) => {
				return Some(variable.clone());
			},

			// Continue search recursively
			None => match self.parent() {
				Some(parent) => return parent.remove_variable(name),

				// Not found
				None => return None,
			}
		}
	}
	*/

	/// Merges [`other`] into [`self`]
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
