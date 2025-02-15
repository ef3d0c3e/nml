use std::borrow::Borrow;
use std::borrow::BorrowMut;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use super::source::Source;
use super::state::ParseMode;
use super::state::ParserState;

use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::document::references::Reference;
use crate::document::references::Refname;
use crate::document::variable::Variable;
use crate::document::variable::VariableName;
use crate::elements::paragraph::elem::Paragraph;
use crate::elements::paragraph::elem::ParagraphToken;

/// The scope from a translation unit
/// Each scope is tied to a unique [`Source`]
#[derive(Debug)]
pub struct Scope {
	/// Stores the element range in the unit's ast
	pub(crate) range: Range<usize>,

	/// Parent scope
	parent: Option<Rc<RefCell<Scope>>>,

	/// Content of this scope
	content: Vec<Arc<dyn Element>>,

	/// State of the parser
	parser_state: super::state::ParserState,

	/// Source of this scope
	source: Arc<dyn Source>,

	/// Declared references in this scope
	references: HashMap<Refname, Rc<Reference>>,

	/// Variables declared within the scope
	variables: HashMap<VariableName, Rc<dyn Variable>>,

	/// Enables paragraphing
	///
	/// Paragraphing should be enabled for default content scopes
	paragraphing: bool,

	/// Currently active paragraph
	active_paragraph: Option<Arc<dyn Element>>,
}

impl Scope {
	pub fn new(
		parent: Option<Rc<RefCell<Scope>>>,
		source: Arc<dyn Source>,
		parse_mode: ParseMode,
		start: usize,
	) -> Self {
		Self {
			range: start..start,
			parent,
			content: vec![],
			parser_state: ParserState::new(parse_mode),
			source,
			references: HashMap::default(),
			variables: HashMap::default(),
			paragraphing: true,
			active_paragraph: None,
		}
	}

	/// The name of this scope (which corresponds to the name of the source)
	pub fn name(&self) -> &String { self.source.name() }

	/// Returns the source of this document
	pub fn source(&self) -> Arc<dyn Source> { self.source.clone() }

	/// Returns the parser's state
	pub fn parser_state(&self) -> &ParserState { &self.parser_state }

	/// Returns a mutable parser state
	pub fn parser_state_mut(&mut self) -> &mut ParserState { &mut self.parser_state }

	/// Inserts a reference in the current scope.
	///
	/// On conflict, returns the conflicting reference.
	pub fn insert_reference(&mut self, reference: Rc<Reference>) -> Option<Rc<Reference>> {
		self.references
			.insert(reference.name().to_owned(), reference)
	}

	/// Inserts a variable in the current scope.
	///
	/// On conflict, returns the conflicting variable.
	pub fn insert_variable(&mut self, variable: Rc<dyn Variable>) -> Option<Rc<dyn Variable>> {
		self.variables.insert(variable.name().to_owned(), variable)
	}
}

pub trait ScopeAccessor {
	fn current_paragraph(&self) -> Option<Arc<dyn Element>>;
	/// Creates a new child from this scope
	fn new_child(
		&self,
		source: Arc<dyn Source>,
		parse_mode: ParseMode,
		visible: bool,
	) -> Rc<RefCell<Scope>>;

	/// Returns a reference as well as it's declaring scope
	fn get_reference(&self, name: &Refname) -> Option<(Rc<Reference>, Rc<RefCell<Scope>>)>;

	/// Returns a variable as well as it's declaring scope
	fn get_variable(&self, name: &VariableName) -> Option<(Rc<dyn Variable>, Rc<RefCell<Scope>>)>;

	/// Should be called by the owning [`TranslationUnit`] to acknowledge an element being added
	fn add_content(&self, elem: Arc<dyn Element>);

	/// Get an element from an id
	fn get_content(&self, id: usize) -> Option<Arc<dyn Element>>;

	/// Gets the last element of this scope
	fn content_last(&self) -> Option<Arc<dyn Element>>;

	/// Gets an iterator over scope elements
	fn content_iter(&self) -> ScopeIterator;
}

impl<'s> ScopeAccessor for Rc<RefCell<Scope>> {
	fn new_child(
		&self,
		source: Arc<dyn Source>,
		parse_mode: ParseMode,
		paragraphing: bool,
	) -> Rc<RefCell<Scope>> {
		let range = (*self.clone()).borrow().range.clone();
		let mut child = Scope::new(Some(self.clone()), source, parse_mode, range.end);
		child.paragraphing = paragraphing;

		Rc::new(RefCell::new(child))
	}

	fn get_variable(&self, name: &VariableName) -> Option<(Rc<dyn Variable>, Rc<RefCell<Scope>>)> {
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

	fn add_content(&self, elem: Arc<dyn Element>) {
		let mut scope = (*self.to_owned()).borrow_mut();
		if !scope.paragraphing
		{
			scope.content.push(elem);
			return;
		}

		// Push paragraph & update state
		if let Some(paragraph) = elem.downcast_ref::<Paragraph>()
		{
			if paragraph.token == ParagraphToken::End
			{
				scope.active_paragraph = None;
			}
			else
			{
				scope.active_paragraph = Some(elem.clone());
			}

			scope.content.push(elem);
			return;
		}


		if elem.kind() == ElemKind::Inline {
			// Add new paragraph
			if scope.active_paragraph.is_none()
			{
				let paragraph = Arc::new(Paragraph {
					location: elem.location().clone(),
					token: ParagraphToken::Start,
				});
				scope.content.push(paragraph);
				scope.active_paragraph = Some(paragraph.clone());
			}
		} else if elem.kind() == ElemKind::Block {
			// Close paragraph
			if scope.active_paragraph.is_some()
			{
				let paragraph = Arc::new(Paragraph {
					location: elem.location().clone(),
					token: ParagraphToken::End,
				});
				scope.content.push(paragraph);
				scope.active_paragraph = None;
			}
		}

		scope.content.push(elem);
	}

	fn get_content(&self, id: usize) -> Option<Arc<dyn Element>> {
		if (*self.clone()).borrow().content.len() <= id {
			return None;
		}
		return Some((*self.clone()).borrow().content[id].clone());
	}

	fn content_last(&self) -> Option<Arc<dyn Element>> {
		return (*self.clone()).borrow().content.last().cloned();
	}

	fn content_iter(&self) -> ScopeIterator { ScopeIterator::new(self.clone()) }

	fn current_paragraph(&self) -> Option<Arc<dyn Element>> {
		return (*self.clone()).borrow().active_paragraph.clone();
	}
}

struct ScopeIterator {
	scope: Rc<RefCell<Scope>>,
	position: Vec<(usize, usize)>,
	depth: Vec<Arc<dyn ContainerElement>>,
}

impl ScopeIterator {
	pub fn new(scope: Rc<RefCell<Scope>>) -> Self {
		Self {
			scope,
			position: vec![(0usize, 0usize); 1],
			depth: vec![],
		}
	}
}

impl Iterator for ScopeIterator {
	type Item = (Rc<RefCell<Scope>>, Arc<dyn Element>);

	fn next(&mut self) -> Option<Self::Item> {
		while let (Some(last_depth), Some((scope_id, last_idx))) =
			(self.depth.last(), self.position.last_mut())
		{
			let scope = last_depth.contained()[*scope_id].clone();
			let scope_len = (*scope.clone()).borrow().content.len();

			if *last_idx < scope_len {
				let elem = (*scope.clone()).borrow().content[*last_idx].clone();
				*last_idx += 1;
				return Some((scope.clone(), elem));
			}

			if *scope_id < last_depth.contained().len() {
				*last_idx = 0;
				*scope_id += 1;
			} else {
				self.depth.pop();
				self.position.pop();
			}
		}

		let scope_len = (*self.scope.clone()).borrow().content.len();
		if self.position[0].1 < scope_len {
			let elem = (*self.scope.clone()).borrow().content[self.position[0].1].clone();
			self.position[0].1 += 1;

			if let Some(_container) = elem.as_container() {
				self.position.push((0, 0));
				self.depth
					.push(unsafe { std::mem::transmute(elem.clone()) });
			}

			return Some((self.scope.clone(), elem));
		}

		return None;
	}
}
