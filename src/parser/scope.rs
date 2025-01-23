use std::{borrow::{Borrow, BorrowMut}, cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

use crate::document::{references::{Reference, Refname}, variable::{Variable, VariableName}};

use super::{parser::Parser, source::Source, state::{ParseMode, ParserState}};

/// The scope from a translation unit
/// Each scope is tied to a unique [`Source`]
#[derive(Debug)]
pub struct Scope {
	/// Parent scope
	parent: Option<Rc<RefCell<Scope>>>,

	/// Children of this scope
	children: Vec<Rc<RefCell<Scope>>>,

	/// State of the parser
	parser_state: super::state::ParserState,

	/// Source of this scope
	source: Arc<dyn Source>,

	/// Declared references in this scope
	references: HashMap<Refname, Rc<Reference>>,

	/// Variables declared within the scope
	variables: HashMap<VariableName, Rc<dyn Variable>>,
}

impl Scope {
	pub fn new(parent: Option<Rc<RefCell<Scope>>>, source: Arc<dyn Source>, parse_mode: ParseMode) -> Self {
		Self {
			parent,
			children: Vec::default(),
			parser_state: ParserState::new(parse_mode),
			source,
			references: HashMap::default(),
			variables: HashMap::default(),
		}
	}
	
	pub fn name(&self) -> &String { self.source.name() }

	pub fn source(&self) -> Arc<dyn Source> {
		self.source.clone()
	}


	pub fn parser_state(&self) -> &ParserState
	{
		&self.parser_state
	}

	pub fn parser_state_mut(&mut self) -> &mut ParserState
	{
		&mut self.parser_state
	}
}

pub trait ScopeAccessor {
	/// Creates a new child from this scope
	fn new_child(&self, source: Arc<dyn Source>, parse_mode: ParseMode) -> Rc<RefCell<Scope>>;

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
}

impl ScopeAccessor for Rc<RefCell<Scope>> {
	fn new_child(&self, source: Arc<dyn Source>, parse_mode: ParseMode) -> Rc<RefCell<Scope>>
	{
		let child = Rc::new(RefCell::new(Scope::new(Some(self.clone()), source, parse_mode)));

		(*self.clone()).borrow_mut().children.push(child.clone());
		child
	}

	fn get_reference(&self, name: &Refname) -> Option<(Rc<Reference>, Rc<RefCell<Scope>>)> {
		if let Some(reference) = (*self.clone()).borrow().references.get(name) {
			return Some((reference.clone(), self.clone()));
		}

		if let Some(parent) = &(*self.clone()).borrow().parent {
			return parent.get_reference(name);
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

		if let Some(parent) = &(*self.clone()).borrow().parent {
			return parent.get_variable(name);
		}

		return None;
	}

	fn insert_variable(&self, variable: Rc<dyn Variable>) -> Option<Rc<dyn Variable>>
	{
		(*self.clone()).borrow_mut()
			.variables.insert(variable.name().to_owned(), variable)
	}
}
