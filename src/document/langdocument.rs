use std::cell::RefCell;
use std::sync::Arc;

use crate::parser::source::Source;

use super::document::Document;
use super::document::Scope;
use super::element::Element;

#[derive(Debug)]
pub struct LangDocument<'a> {
	source: Arc<dyn Source>,
	parent: Option<&'a dyn Document<'a>>,
	/// Document's parent
	// FIXME: Render these fields private
	pub content: RefCell<Vec<Box<dyn Element>>>,
	pub scope: RefCell<Scope>,
}

impl<'a> LangDocument<'a> {
	pub fn new(source: Arc<dyn Source>, parent: Option<&'a dyn Document<'a>>) -> Self {
		Self {
			source,
			parent,
			content: RefCell::new(Vec::new()),
			scope: RefCell::new(Scope::new()),
		}
	}
}

impl<'a> Document<'a> for LangDocument<'a> {
	fn source(&self) -> Arc<dyn Source> { self.source.clone() }

	fn parent(&self) -> Option<&'a dyn Document<'a>> { self.parent.map(|p| p as &dyn Document<'a>) }

	fn content(&self) -> &RefCell<Vec<Box<dyn Element>>> { &self.content }

	fn scope(&self) -> &RefCell<Scope> { &self.scope }
}
