use std::{borrow::BorrowMut, cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use crate::document::{references::{Reference, Refname}, variable::{Variable, VariableName}};

use super::source::Source;

/// The scope from a translation unit
#[derive(Debug)]
pub struct Scope {
	/// Scope shadowed by this scope
	shadowed: Option<Rc<RefCell<Scope>>>,

	/// Source of this scope
	source: Arc<dyn Source>,

	/// Declared references in this scope
	references: HashMap<Refname, Rc<Reference>>,

	/// Variables declared within the scope
	variables: HashMap<VariableName, Rc<dyn Variable>>,
}

impl Scope {
	pub fn new(source: Arc<dyn Source>) -> Self {
		Self {
			shadowed: None,
			source,
			references: HashMap::default(),
			variables: HashMap::default(),
		}
	}
	
	pub fn name(&self) -> &String { self.source.name() }
}

pub trait ScopeAccessor {
	/// Returns a reference as well as it's declaring scope
	fn get_reference(&self, name: &Refname) -> Option<(Rc<Reference>, Rc<RefCell<Scope>>)>;

	/// Inserts a reference in the current scope.
	///
	/// On conflict, returns the conflicting reference.
	fn insert_reference(&self, reference: Rc<Reference>) -> Option<Rc<Reference>>;

	/// Returns a variable as well as it's declaring scope
	fn get_variable(
		&self,
		name: &VariableName,
	) -> Option<(Rc<dyn Variable>, Rc<RefCell<Scope>>)>;

	/// Inserts a variable in the current scope.
	///
	/// On conflict, returns the conflicting variable.
	fn insert_variable(&self, variable: Rc<dyn Variable>) -> Option<Rc<dyn Variable>>;

	/// Creates a new scope that shadows the current scope
	fn shadows(self, source: Arc<dyn Source>) -> Rc<RefCell<Scope>>;
}

impl ScopeAccessor for Rc<RefCell<Scope>> {
	fn get_reference(&self, name: &Refname) -> Option<(Rc<Reference>, Rc<RefCell<Scope>>)> {
		if let Some(reference) = (*self.clone()).borrow().references.get(name) {
			return Some((reference.clone(), self.clone()));
		}

		if let Some(shadowed) = &(*self.clone()).borrow().shadowed {
			return shadowed.get_reference(name);
		}

		return None;
	}

	fn insert_reference(&self, reference: Rc<Reference>) -> Option<Rc<Reference>>
	{
		(*self.clone()).borrow_mut()
			.references.insert(reference.name().to_owned(), reference)
	}

	fn get_variable(
		&self,
		name: &VariableName,
	) -> Option<(Rc<dyn Variable>, Rc<RefCell<Scope>>)> {
		if let Some(variable) = (*self.clone()).borrow().variables.get(name) {
			return Some((variable.clone(), self.clone()));
		}

		if let Some(shadowed) = &(*self.clone()).borrow().shadowed {
			return shadowed.get_variable(name);
		}

		return None;
	}

	fn insert_variable(&self, variable: Rc<dyn Variable>) -> Option<Rc<dyn Variable>>
	{
		(*self.clone()).borrow_mut()
			.variables.insert(variable.name().to_owned(), variable)
	}

	fn shadows(self, source: Arc<dyn Source>) -> Rc<RefCell<Scope>> {
		let scope = Rc::new(RefCell::new(Scope::new(source)));
		(*self.clone()).borrow_mut().shadowed = Some(scope.clone());
		scope
	}
}
