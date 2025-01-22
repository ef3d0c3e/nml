use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::document::element::Element;

use super::scope::Scope;
use super::scope::ScopeAccessor;
use super::source::Source;

/// Stores the data required by the parser
pub struct TranslationUnit {
	/// Entry point of this translation unit
	source: Arc<dyn Source>,
	/// Resulting AST
	/// Elements are stored using Arc so they can be passed to an async task
	content: Vec<Arc<dyn Element>>,
	/// Scope of this translation unit
	scope: Rc<RefCell<Scope>>,
}

impl TranslationUnit {
	/// Creates a new translation unit
	///
	/// Should be called once for each distinct source file
	pub fn new(source: Arc<dyn Source>) -> Self
	{
		Self {
			source: source.clone(),
			content: vec![],
			scope: Rc::new(RefCell::new(Scope::new(source)))
		}
	}

	/// Runs procedure with a newly created scope from a source file
	pub fn with_scope<F, R>(&mut self, source: Arc<dyn Source>, f: F) -> R
	where
		F: FnOnce(Rc<RefCell<Scope>>) -> R,
	{
		self.scope = self.scope.clone().shadows(source);
		f(self.scope.clone())
	}
}
