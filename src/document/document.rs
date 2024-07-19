use std::cell::{Ref, RefCell, RefMut};
use std::collections::hash_map::HashMap;
use std::rc::Rc;

use crate::parser::source::Source;

use super::element::Element;
use super::variable::Variable;


#[derive(Debug)]
pub struct Scope {
    /// List of all referenceable elements in current scope.
    /// All elements in this should return a non empty 
	pub referenceable: HashMap<String, usize>,
	pub variables: HashMap<String, Rc<dyn Variable>>,
}

impl Scope {
	fn new() -> Self {
		Self {
			referenceable: HashMap::new(),
			variables: HashMap::new(),
		}
	}

    pub fn merge(&mut self, other: &mut Scope, merge_as: &String, ref_offset: usize)
    {
        match merge_as.is_empty()
        {
            true => {
                // References
                self.referenceable.extend(other.referenceable.drain()
                    .map(|(name, idx)|
                        (name, idx+ref_offset)));

                // Variables
                self.variables.extend(other.variables.drain()
                    .map(|(name, var)|
                        (name, var)));
            },
            false => {
                // References
                self.referenceable.extend(other.referenceable.drain()
                    .map(|(name, idx)|
                        (format!("{merge_as}.{name}"), idx+ref_offset)));

                // Variables
                self.variables.extend(other.variables.drain()
                    .map(|(name, var)|
                        (format!("{merge_as}.{name}"), var)));
            }
        }
    }
}

#[derive(Debug)]
pub struct Document<'a> {
	source: Rc<dyn Source>,
	parent: Option<&'a Document<'a>>, /// Document's parent

	// FIXME: Render these fields private
	pub content: RefCell<Vec<Box<dyn Element>>>,
	pub scope: RefCell<Scope>,
}

impl<'a> Document<'a>  {
	pub fn new(source: Rc<dyn Source>, parent: Option<&'a Document<'a>>) -> Self
	{
		Self {
			source: source,
			parent: parent,
			content: RefCell::new(Vec::new()),
			scope: RefCell::new(Scope::new()),
		}
	}

	pub fn source(&self) -> Rc<dyn Source> { self.source.clone() }

    pub fn parent(&self) -> Option<&Document> { self.parent }

    /// Push an element [`elem`] to content. [`in_paragraph`] is true if a paragraph is active
	pub fn push(&self, elem: Box<dyn Element>)
	{
        // Add index of current element to scope's reference table
        if let Some(referenceable) = elem.as_referenceable()
        {
            // Only add if referenceable holds a reference
            if let Some(ref_name) = referenceable.reference_name()
            {
                self.scope.borrow_mut().referenceable.insert(ref_name.clone(), self.content.borrow().len());
            }
        }

		self.content.borrow_mut().push(elem);
	}

	pub fn last_element<T: Element>(&self, recurse: bool) -> Option<Ref<'_, T>>
	{
		let elem = Ref::filter_map(self.content.borrow(),
		|content| content.last()
			.and_then(|last| last.downcast_ref::<T>())).ok();

		if elem.is_some() || !recurse { return elem }

		match self.parent
		{
			None => None,
			Some(parent) => parent.last_element(true),
		}
	}

	pub fn last_element_mut<T: Element>(&self, recurse: bool) -> Option<RefMut<'_, T>>
	{
		let elem = RefMut::filter_map(self.content.borrow_mut(),
		|content| content.last_mut()
			.and_then(|last| last.downcast_mut::<T>())).ok();

		if elem.is_some() || !recurse { return elem }

		match self.parent
		{
			None => None,
			Some(parent) => parent.last_element_mut(true),
		}
	}

	pub fn get_reference(&self, ref_name: &str) -> Option<(&Document<'a>, std::cell::Ref<'_, Box<dyn Element>>)> {
		match self.scope.borrow().referenceable.get(ref_name) {
			// Return if found
			Some(elem) => {
                return Some((&self,
                    std::cell::Ref::map(self.content.borrow(),
                    |m| &m[*elem])))
            },

			// Continue search recursively
			None => match self.parent {
				Some(parent) => return parent.get_reference(ref_name),

				// Not found
				None => return None,
			}
		}
	}

    pub fn add_variable(&self, variable: Rc<dyn Variable>)
    {
        self.scope.borrow_mut().variables.insert(
            variable.name().to_string(),
            variable);
    }

    pub fn get_variable<S: AsRef<str>>(&self, name: S) -> Option<(&Document<'a>, Rc<dyn Variable>)>
    {
        match self.scope.borrow().variables.get(name.as_ref())
        {
            Some(variable) => {
                return Some((&self, variable.clone()));
            },

			// Continue search recursively
            None => match self.parent {
                Some(parent) => return parent.get_variable(name),

                // Not found
                None => return None,
            }
        }
    }

	pub fn remove_variable<S: AsRef<str>>(&self, name: S) -> Option<(&Document<'a>, Rc<dyn Variable>)>
	{
        match self.scope.borrow_mut().variables.remove(name.as_ref())
        {
            Some(variable) => {
                return Some((&self, variable.clone()));
            },

			// Continue search recursively
            None => match self.parent {
                Some(parent) => return parent.remove_variable(name),

                // Not found
                None => return None,
            }
        }
	}

    /// Merges [`other`] into [`self`]
    pub fn merge(&self, other: Document, merge_as: Option<&String>)
    {
		match merge_as
		{
			Some(merge_as)	=> self.scope.borrow_mut()
				.merge(
					&mut *other.scope.borrow_mut(),
					merge_as,
					self.content.borrow().len()+1),
			_ => {},
		}

        // Content
        self.content.borrow_mut().extend((other.content.borrow_mut())
            .drain(..)
            .map(|value| value));
    }
}


