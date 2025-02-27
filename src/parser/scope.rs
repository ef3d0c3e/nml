use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use super::source::Source;
use super::state::ParseMode;
use super::state::ParserState;

use crate::document::element::ContainerElement;
use crate::document::element::Element;
use crate::document::references::InternalReference;
use crate::document::references::Refname;
use crate::document::variable::Variable;
use crate::document::variable::VariableName;

/// The scope from a translation unit
/// Each scope is tied to a unique [`Source`]
pub struct Scope {
	/// Stores the element range in the unit's ast
	pub(crate) range: Range<usize>,

	/// Parent scope
	parent: Option<Rc<RefCell<Scope>>>,

	/// Content of this scope
	content: Vec<Rc<dyn Element>>,

	/// State of the parser
	parser_state: super::state::ParserState,

	/// Source of this scope
	source: Arc<dyn Source>,

	/// Declared references in this scope
	references: HashMap<Refname, Rc<InternalReference>>,

	/// Variables declared within the scope
	variables: HashMap<VariableName, Rc<dyn Variable>>,

	/// Enables paragraphing
	///
	/// Paragraphing should be enabled for default content scopes
	paragraphing: bool,
}

impl core::fmt::Debug for Scope
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {

        write!(f, "Scope{{\n\tcontent {:#?}\nrange {:#?}\nsource: {:#?}}}", self.content, self.range, self.source)
    }
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
}

pub trait ScopeAccessor {
	/// Creates a new child from this scope
	fn new_child(
		&self,
		source: Arc<dyn Source>,
		parse_mode: ParseMode,
		visible: bool,
	) -> Rc<RefCell<Scope>>;

	/// Returns an internal reference as well as it's declaring scope
	fn get_reference(&self, name: &Refname) -> Option<(Rc<InternalReference>, Rc<RefCell<Scope>>)>;

	/// Inserts an internal reference
	fn insert_reference(&self, reference: Rc<InternalReference>) -> Option<Rc<InternalReference>>;

	/// Returns a variable as well as it's declaring scope
	fn get_variable(&self, name: &VariableName) -> Option<(Rc<dyn Variable>, Rc<RefCell<Scope>>)>;

	/// Inserts a variable
	fn insert_variable(&self, var: Rc<dyn Variable>) -> Option<Rc<dyn Variable>>;

	/// Should be called by the owning [`TranslationUnit`] to acknowledge an element being added
	fn add_content(&self, elem: Rc<dyn Element>);

	/// Get an element from an id
	fn get_content(&self, id: usize) -> Option<Rc<dyn Element>>;

	/// Gets the last element of this scope
	fn content_last(&self) -> Option<Rc<dyn Element>>;

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
		// Close active paragraph
		//if (*self.clone()).borrow().active_paragraph.is_some()
		//{
		//	let elem = {
		//		let rc_ref : Rc<RefCell<Scope>> = self.to_owned();
		//		let scope : std::cell::Ref<Scope> = (*rc_ref).borrow();
		//		Arc::new(Paragraph {
		//			location: Token::new(scope.range.end..scope.range.end, scope.source.clone()),
		//			token: ParagraphToken::End,
		//		})
		//	};
		//	self.add_content(elem)
		//}

		let range = (*self.clone()).borrow().range.clone();
		let mut child = Scope::new(Some(self.clone()), source, parse_mode, range.end);
		child.paragraphing = paragraphing;

		Rc::new(RefCell::new(child))
	}

	fn insert_reference(&self, reference: Rc<InternalReference>) -> Option<Rc<InternalReference>>
	{
		let mut scope = Rc::as_ref(self).borrow_mut();
		scope.references.insert(reference.name().to_owned(), reference)
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

	fn insert_variable(&self, var: Rc<dyn Variable>) -> Option<Rc<dyn Variable>>
	{
		let mut scope = Rc::as_ref(self).borrow_mut();
		scope.variables.insert(var.name().to_owned(), var)
	}

	fn get_reference(&self, name: &Refname) -> Option<(Rc<InternalReference>, Rc<RefCell<Scope>>)> {
		if let Some(reference) = (*self.clone()).borrow().references.get(name) {
			return Some((reference.clone(), self.clone()));
		}

		if let Some(parent) = &(*self.clone()).borrow().parent {
			return parent.get_reference(name);
		}

		return None;
	}

	fn add_content(&self, elem: Rc<dyn Element>) {
		let mut scope = Rc::as_ref(self).borrow_mut();
		assert_eq!(*elem.location().source(), *scope.source);
		scope.content.push(elem);
	}

	fn get_content(&self, id: usize) -> Option<Rc<dyn Element>> {
		if (*self.clone()).borrow().content.len() <= id {
			return None;
		}
		return Some((*self.clone()).borrow().content[id].clone());
	}

	fn content_last(&self) -> Option<Rc<dyn Element>> {
		return (*self.clone()).borrow().content.last().cloned();
	}

	fn content_iter(&self) -> ScopeIterator { ScopeIterator::new(self.clone()) }
}

/// DFS iterator for the syntax tree
pub struct ScopeIterator {
	scope: Rc<RefCell<Scope>>,
	position: Vec<(usize, usize)>,
	depth: Vec<Rc<dyn ContainerElement>>,
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
	type Item = (Rc<RefCell<Scope>>, Rc<dyn Element>);

	fn next(&mut self) -> Option<Self::Item> {
		// Pop at the end of scope
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

			if *scope_id + 1 < last_depth.contained().len() {
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

			if let Some(container) = elem.clone().as_container() {

				self.position.push((0, 0));
				self.depth
					.push(container);
			}

			return Some((self.scope.clone(), elem));
		}

		return None;
	}
}
