use std::{cell::RefCell, collections::HashMap, ops::Range, rc::Rc};

use ariadne::Report;
use downcast_rs::{impl_downcast, Downcast};

use crate::document::document::Document;

use super::{parser::Parser, source::Source};

/// Scope for state objects
#[derive(PartialEq, PartialOrd)]
pub enum Scope
{
	/// Global state
	GLOBAL = 0,
	/// Document-local state
	DOCUMENT = 1,
	/// Paragraph-local state
	/// NOTE: Even though paragraph may span across multiple documents,
	/// a paragraph-local state should be removed when importing a new document
	PARAGRAPH = 2,
}

pub trait State: Downcast
{
	/// Returns the state's [`Scope`]
	fn scope(&self) -> Scope;

	/// Callback called when state goes out of scope
	fn on_remove<'a>(&self, parser: &dyn Parser, document: &Document) -> Vec<Report<'a, (Rc<dyn Source>, Range<usize>)>>;
}
impl_downcast!(State);

/// Object owning all the states
pub struct StateHolder
{
	data: HashMap<String, Rc<RefCell<dyn State>>>
}

impl StateHolder
{
	pub fn new() -> Self {
		Self {
			data: HashMap::new(),
		}
	}

	// Attempts to push [`state`]. On collision, returns an error with the already present state
	pub fn insert(&mut self, name: String, state: Rc<RefCell<dyn State>>) -> Result<Rc<RefCell<dyn State>>, Rc<RefCell<dyn State>>>
	{
		match self.data.insert(name, state.clone())
		{
			Some(state) => Err(state),
			_ => Ok(state)
		}
	}

	pub fn query(&self, name: &String) -> Option<Rc<RefCell<dyn State>>>
	{
		self.data
			.get(name)
			.map_or(None, |st| Some(st.clone()))
	}

	pub fn on_scope_end(&mut self, parser: &dyn Parser, document: &Document, scope: Scope) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>
	{
		let mut result = vec![];
		
		self.data
			.retain(|_name, state|
			{
				if state.borrow().scope() >= scope
				{
					state.borrow().on_remove(parser, document)
						.drain(..)
						.for_each(|report| result.push(report));
					false
				}
				else
				{
					true
				}
			});

		return result;
	}
}
