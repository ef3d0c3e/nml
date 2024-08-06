use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use ariadne::Report;
use downcast_rs::impl_downcast;
use downcast_rs::Downcast;

use crate::document::document::Document;

use super::parser::ParserState;
use super::source::Source;

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
	fn on_remove<'a>(
		&self,
		state: &ParserState,
		document: &dyn Document,
	) -> Vec<Report<'a, (Rc<dyn Source>, Range<usize>)>>;
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
		self.states.get(state_name).map(|state| state.clone())
	}

	pub fn on_scope_end(
		&mut self,
		state: &ParserState,
		document: &dyn Document,
		scope: Scope,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
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

		return reports;
	}
}
