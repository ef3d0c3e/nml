use std::{borrow::{Borrow, BorrowMut}, cell::{RefCell, RefMut}, collections::HashMap, ops::{Deref, Range}, rc::Rc, sync::Arc};

use crate::document::{references::{Reference, Refname}, variable::{Variable, VariableName}};

use super::{parser::Parser, source::Source, state::{ParseMode, ParserState}};

/// The scope from a translation unit
/// Each scope is tied to a unique [`Source`]
#[derive(Debug)]
pub struct Scope {
	/// Stores the element range in the unit's ast
	pub(crate) range: Range<usize>,

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
	pub fn new(parent: Option<Rc<RefCell<Scope>>>, source: Arc<dyn Source>, parse_mode: ParseMode, start: usize) -> Self {
		Self {
			range: start..start,
			parent,
			children: Vec::default(),
			parser_state: ParserState::new(parse_mode),
			source,
			references: HashMap::default(),
			variables: HashMap::default(),
		}
	}
	
	/// The name of this scope (which corresponds to the name of the source)
	pub fn name(&self) -> &String { self.source.name() }

	/// Returns the source of this document
	pub fn source(&self) -> Arc<dyn Source> {
		self.source.clone()
	}

	/// Returns the parser's state
	pub fn parser_state(&self) -> &ParserState
	{
		&self.parser_state
	}

	/// Returns a mutable parser state
	pub fn parser_state_mut(&mut self) -> &mut ParserState
	{
		&mut self.parser_state
	}


	/// Inserts a reference in the current scope.
	///
	/// On conflict, returns the conflicting reference.
	pub fn insert_reference(&mut self, reference: Rc<Reference>) -> Option<Rc<Reference>>
	{
		self	
			.references.insert(reference.name().to_owned(), reference)
	}

	/// Inserts a variable in the current scope.
	///
	/// On conflict, returns the conflicting variable.
	pub fn insert_variable(&mut self, variable: Rc<dyn Variable>) -> Option<Rc<dyn Variable>>
	{
		self
			.variables.insert(variable.name().to_owned(), variable)
	}
}

pub trait ScopeAccessor {
	/// Creates a new child from this scope
	fn new_child(&self, source: Arc<dyn Source>, parse_mode: ParseMode) -> Rc<RefCell<Scope>>;

	/// Returns a reference as well as it's declaring scope
	fn get_reference(&self, name: &Refname) -> Option<(Rc<Reference>, Rc<RefCell<Scope>>)>;

	/// Returns a variable as well as it's declaring scope
	fn get_variable(
		&self,
		name: &VariableName,
	) -> Option<(Rc<dyn Variable>, Rc<RefCell<Scope>>)>;

	/// Should be called by the owning [`TranslationUnit`] to acknowledge an element being added
	fn add_content(&self);
}

impl<'s> ScopeAccessor for Rc<RefCell<Scope>> {
	fn new_child(&self, source: Arc<dyn Source>, parse_mode: ParseMode) -> Rc<RefCell<Scope>>
	{
		let range = (*self.clone()).borrow().range.clone();
		let child = Rc::new(RefCell::new(Scope::new(Some(self.clone()), source, parse_mode, range.end)));

		(*self.clone()).borrow_mut().children.push(child.clone());
		child
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

	fn get_reference(&self, name: &Refname) -> Option<(Rc<Reference>, Rc<RefCell<Scope>>)> {
		if let Some(reference) = (*self.clone()).borrow().references.get(name) {
			return Some((reference.clone(), self.clone()));
		}

		if let Some(parent) = &(*self.clone()).borrow().parent {
			return parent.get_reference(name);
		}

		return None;
	}

	fn add_content(&self) {
		let scope = self.clone();
		let mut borrow = (*scope).borrow_mut();
		borrow.range.end += 1;

		// Propagate to parent
		if let Some(parent) = &borrow.parent
		{
			parent.add_content();
		}
	}
}

