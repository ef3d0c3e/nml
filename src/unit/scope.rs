use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use parking_lot::MappedRwLockWriteGuard;
use parking_lot::RwLock;
use parking_lot::RwLockWriteGuard;

use crate::parser::reports::Report;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::state::CustomState;
use crate::parser::state::ParseMode;
use crate::parser::state::ParserState;

use super::element::ContainerElement;
use super::element::Element;
use super::translation::TranslationUnit;
use super::variable::Variable;
use super::variable::VariableName;
use super::variable::VariableVisibility;

/// The scope from a translation unit
/// Each scope is tied to a unique [`Source`]
pub struct Scope {
	/// Stores the element range in the unit's ast
	range: Range<usize>,

	/// Parent scope
	parent: Option<Arc<RwLock<Scope>>>,

	/// Content of this scope
	content: Vec<Arc<dyn Element>>,

	/// State of the parser
	parser_state: ParserState,

	/// Source of this scope
	source: Arc<dyn Source>,

	/// Variables declared within the scope
	pub variables: HashMap<VariableName, Arc<dyn Variable>>,

	/// Enables paragraphing
	///
	/// Paragraphing should be enabled for default content scopes
	paragraphing: bool,
}

impl core::fmt::Debug for Scope {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"Scope{{\n\tcontent {:#?}\nrange {:#?}\nsource: {:#?}}}",
			self.content, self.range, self.source
		)
	}
}

impl Scope {
	pub fn new(
		parent: Option<Arc<RwLock<Scope>>>,
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
			variables: HashMap::default(),
			paragraphing: true,
		}
	}

	/// The name of this scope (which corresponds to the name of the source)
	pub fn name(&self) -> &String {
		self.source.name()
	}

	/// Returns the source of this scope
	pub fn source(&self) -> Arc<dyn Source> {
		self.source.clone()
	}

	/// Returns the parser's state
	pub fn parser_state(&self) -> &ParserState {
		&self.parser_state
	}

	/// Returns a mutable parser state
	pub fn parser_state_mut(&mut self) -> &mut ParserState {
		&mut self.parser_state
	}

	/// Sets the parser state for this scope
	pub fn set_parser_state(&mut self, parser_state: ParserState) {
		self.parser_state = parser_state;
	}

	pub fn parent(&self) -> &Option<Arc<RwLock<Scope>>> {
		&self.parent
	}
}

pub trait ScopeAccessor {
	/// Creates a new child from this scope
	fn new_child(
		&self,
		source: Arc<dyn Source>,
		parse_mode: ParseMode,
		visible: bool,
	) -> Arc<RwLock<Scope>>;

	/// Method called when the scope ends
	fn on_end(&self, unit: &mut TranslationUnit, document: bool) -> Vec<Report>;

	/// Returns a variable as well as it's declaring scope
	fn get_variable(&self, name: &VariableName) -> Option<(Arc<dyn Variable>, Arc<RwLock<Scope>>)>;

	/// Inserts a variable
	fn insert_variable(&self, var: Arc<dyn Variable>) -> Option<Arc<dyn Variable>>;

	/// Should be called by the owning [`TranslationUnit`] to acknowledge an element being added
	fn add_content(&self, elem: Arc<dyn Element>);

	/// Get an element from an id
	fn get_content(&self, id: usize) -> Option<Arc<dyn Element>>;

	/// Gets the last element of this scope
	fn content_last(&self) -> Option<Arc<dyn Element>>;

	/// Gets an iterator over scope elements
	///
	/// When recurse is enabled, recursively contained scopes will be iterated.
	/// Otherwise only the content of the current scope will be iterated.
	fn content_iter(&self, recurse: bool) -> ScopeIterator;

	/// Add an imported scope to this scope
	/// This will insert the variables defined within `imported`
	/// into `self`
	fn add_import(&self, imported: Arc<RwLock<Scope>>);

	fn has_state(&self, name: &str) -> bool;

	fn with_state<T, F, R>(&self, name: &str, f: F) -> R
	where
		T: CustomState,
		F: FnOnce(MappedRwLockWriteGuard<'_, T>) -> R;

	fn token(&self) -> Token;
}

impl<'s> ScopeAccessor for Arc<RwLock<Scope>> {
	fn new_child(
		&self,
		source: Arc<dyn Source>,
		parse_mode: ParseMode,
		paragraphing: bool,
	) -> Arc<RwLock<Scope>> {
		let range = (*self.clone()).read().range.clone();
		let mut child = Scope::new(Some(self.clone()), source, parse_mode, range.end);
		child.paragraphing = paragraphing;

		Arc::new(RwLock::new(child))
	}

	fn on_end(&self, unit: &mut TranslationUnit, document: bool) -> Vec<Report> {
		let states = {
			let mut scope = self.write();
			std::mem::replace(&mut scope.parser_state.states, HashMap::default())
		};
		let mut reports = vec![];
		states.iter().for_each(|(_, state)| {
			let mut lock = state.write();
			if document {
				reports.extend(lock.on_document_end(unit, self.clone()));
			} else {
				reports.extend(lock.on_scope_end(unit, self.clone()));
			}
		});
		reports
	}

	fn get_variable(&self, name: &VariableName) -> Option<(Arc<dyn Variable>, Arc<RwLock<Scope>>)> {
		if let Some(variable) = (*self.clone()).read().variables.get(name) {
			return Some((variable.clone(), self.clone()));
		}

		if let Some(parent) = &(*self.clone()).read().parent {
			return parent.get_variable(name);
		}

		return None;
	}

	fn insert_variable(&self, var: Arc<dyn Variable>) -> Option<Arc<dyn Variable>> {
		let mut scope = Arc::as_ref(self).write();
		scope.variables.insert(var.name().to_owned(), var)
	}

	fn add_content(&self, elem: Arc<dyn Element>) {
		let mut scope = Arc::as_ref(self).write();
		assert_eq!(*elem.location().source(), *scope.source);
		scope.range.end = elem.location().end();
		scope.content.push(elem);
	}

	fn get_content(&self, id: usize) -> Option<Arc<dyn Element>> {
		if (*self.clone()).read().content.len() <= id {
			return None;
		}
		return Some((*self.clone()).read().content[id].clone());
	}

	fn content_last(&self) -> Option<Arc<dyn Element>> {
		return (*self.clone()).read().content.last().cloned();
	}

	fn content_iter(&self, recurse: bool) -> ScopeIterator {
		ScopeIterator::new(self.clone(), recurse)
	}

	fn add_import(&self, imported: Arc<RwLock<Scope>>) {
		let borrow = imported.read();
		borrow.variables.iter().for_each(|(_, var)| {
			if *var.visibility() == VariableVisibility::Exported {
				self.insert_variable(var.clone());
			}
		});
	}

	fn has_state(&self, name: &str) -> bool {
		let borrow = self.read();
		borrow.parser_state.states.contains_key(name)
	}

	fn with_state<T, F, R>(&self, name: &str, f: F) -> R
	where
		T: CustomState,
		F: FnOnce(MappedRwLockWriteGuard<'_, T>) -> R,
	{
		let map = self.read();
		let state = map.parser_state.states.get(name).unwrap();
		let lock = state.write();
		let mapped = RwLockWriteGuard::map(lock, |data| {
			data.as_any_mut()
				.downcast_mut::<T>()
				.expect("Mismatch data types")
		});
		f(mapped)
	}

	fn token(&self) -> Token {
		let scope = self.read();
		Token::new(scope.range.clone(), scope.source.clone())
	}
}

/// DFS iterator for the syntax tree
pub struct ScopeIterator {
	scope: Arc<RwLock<Scope>>,
	position: Vec<(usize, usize)>,
	depth: Vec<Arc<dyn ContainerElement>>,
	recurse: bool,
}

impl ScopeIterator {
	pub fn new(scope: Arc<RwLock<Scope>>, recurse: bool) -> Self {
		Self {
			scope,
			position: vec![(0usize, 0usize); 1],
			depth: vec![],
			recurse,
		}
	}
}

impl Iterator for ScopeIterator {
	type Item = (Arc<RwLock<Scope>>, Arc<dyn Element>);

	fn next(&mut self) -> Option<Self::Item> {
		// Pop at the end of scope
		while let (Some(last_depth), Some((cur_scope, cur_pos))) =
			(self.depth.last(), self.position.last_mut())
		{
			if last_depth.contained().is_empty() {
				self.depth.pop();
				self.position.pop();
				continue;
			}
			let scope = last_depth.contained()[*cur_scope].clone();
			let scope_len = (*scope.clone()).read().content.len();

			if *cur_pos < scope_len {
				let elem = (*scope.clone()).read().content[*cur_pos].clone();
				*cur_pos += 1;
				return Some((scope.clone(), elem));
			}

			// Check if there are more contained scopes to iterate
			if *cur_scope + 1 < last_depth.contained().len() {
				*cur_pos = 0;
				*cur_scope += 1;
			} else {
				self.depth.pop();
				self.position.pop();
			}
		}

		let scope_len = (*self.scope.clone()).read().content.len();
		let cur_pos = &mut self.position[0].1;
		if *cur_pos < scope_len {
			let elem = (*self.scope.clone()).read().content[*cur_pos].clone();
			*cur_pos += 1;

			if self.recurse {
				if let Some(container) = elem.clone().as_container() {
					self.position.push((0, 0));
					self.depth.push(container);
				}
			}

			return Some((self.scope.clone(), elem));
		}

		return None;
	}
}
