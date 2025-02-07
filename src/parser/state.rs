use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use downcast_rs::impl_downcast;
use downcast_rs::Downcast;

use super::translation::TranslationUnit;

/// Modifies the parser's behaviour
///
/// This is useful when the parser is invoked recursively as it can modify how the parser
/// processes text.
#[derive(Default, Debug)]
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
pub enum StateScope {
	/// Global state
	Global = 0,
	/// Scope-local state
	Scope = 1,
	/// Paragraph-local state
	Paragraph = 2,
}

pub trait RuleState: Downcast {
	/// Returns the state's [`Scope`]
	fn scope(&self) -> StateScope;

	/// Callback called when state goes out of scope
	fn on_remove<'u>(&self, unit: &mut TranslationUnit<'u>);
}
impl_downcast!(RuleState);

impl core::fmt::Debug for dyn RuleState {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "State{{Scope: {:#?}}}", self.scope())
	}
}

/// Object owning all active states
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

	pub fn get<S: AsRef<str>>(&self, name: S) -> Option<Rc<RefCell<dyn RuleState>>> {
		self.states.get(name.as_ref()).cloned()
	}

	/// Method called when the current [`StateScope`] ends.
	///
	/// Calling this methods will call into handlers for states going out of scopes
	pub fn on_scope_end<'u>(
		&mut self,
		unit: &mut TranslationUnit<'u>,
		scope: StateScope,
	) {
		self.states.retain(|_name, rule_state| {
			if rule_state.borrow().scope() >= scope {
				rule_state
					.borrow_mut()
					.on_remove(unit);
				false
			} else {
				true
			}
		});
	}
}
