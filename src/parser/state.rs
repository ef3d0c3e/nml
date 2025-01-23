use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use downcast_rs::impl_downcast;
use downcast_rs::Downcast;

use crate::document::document::Document;

use super::parser::Parser;
use super::reports::Report;

/// Modifies the parser's behaviour
///
/// This is useful when the parser is invoked recursively as it can modify how the parser
/// processes text.
#[derive(Default)]
pub struct ParseMode {
	/// Sets the parser to only parse elements compatible with paragraphs.
	pub paragraph_only: bool,
}

/// State for the parser that needs to hold data to parse the current scope
#[derive(Debug)]
pub struct ParserState {
	/// Stores the match data, with the next match position and the data to pass to the processing function
	pub matches: Vec<(usize, Option<Box<dyn Any>>)>,
	/// Current mode for the parser
	pub mode: ParseMode,
}

impl ParserState {
	pub fn new(mode: ParseMode) -> Self {
		Self {
			matches: Vec::default(),
			mode,
		}
	}

	pub fn new_child(&self, mode: ParseMode) -> Self {
		Self::new(mode)
	}
}


// ----------- REFACTOR BELOW ------------

/// Scope for state objects
#[derive(PartialEq, PartialOrd, Debug)]
pub enum Scope {
	/// Global state
	GLOBAL = 0,
	/// Document-local state
	DOCUMENT = 1,
	/// Paragraph-local state
	/// NOTE: Even though paragraph may span across multiple documents,
	/// a paragraph-local state should be removed when importing a new document
	PARAGRAPH = 2,
}

pub trait RuleState: Downcast {
	/// Returns the state's [`Scope`]
	fn scope(&self) -> Scope;

	/// Callback called when state goes out of scope
	fn on_remove(&self, state: &super::parser::ParserState, document: &dyn Document) -> Vec<Report>;
}
impl_downcast!(RuleState);

impl core::fmt::Debug for dyn RuleState {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "State{{Scope: {:#?}}}", self.scope())
	}
}

/// Object owning all the states
#[derive(Default)]
pub struct RuleStateHolder {
	states: HashMap<String, Rc<RefCell<dyn RuleState>>>,
}

impl RuleStateHolder {
	pub fn insert(
		&mut self,
		name: String,
		state: Rc<RefCell<dyn RuleState>>,
	) -> Result<Rc<RefCell<dyn RuleState>>, String> {
		if self.states.contains_key(name.as_str()) {
			return Err(format!("Attempted to insert duplicate RuleState: {name}"));
		}
		self.states.insert(name, state.clone());
		Ok(state)
	}

	pub fn get(&self, state_name: &str) -> Option<Rc<RefCell<dyn RuleState>>> {
		self.states.get(state_name).cloned()
	}

	pub fn on_scope_end(
		&mut self,
		state: &super::parser::ParserState,
		document: &dyn Document,
		scope: Scope,
	) -> Vec<Report> {
		let mut reports = vec![];

		self.states.retain(|_name, rule_state| {
			if rule_state.borrow().scope() >= scope {
				rule_state
					.borrow_mut()
					.on_remove(state, document)
					.drain(..)
					.for_each(|report| reports.push(report));
				false
			} else {
				true
			}
		});

		reports
	}
}
