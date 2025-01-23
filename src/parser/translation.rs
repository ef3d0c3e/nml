use std::cell::Ref;
use std::cell::RefCell;
use std::cell::RefMut;
use std::rc::Rc;
use std::sync::Arc;

use crate::document::element::Element;
use crate::lsp::data::LangServerData;

use super::new::Parser;
use super::parser::ParserState;
use super::parser::ReportColors;
use super::reports::Report;
use super::scope::Scope;
use super::scope::ScopeAccessor;
use super::source::Source;
use super::state::ParseMode;

/// Stores the data required by the parser
pub struct TranslationUnit<'u> {
	/// Parser for this translation unit
	parser: &'u Parser,
	/// Entry point of this translation unit
	source: Arc<dyn Source>,
	/// Resulting AST
	/// Elements are stored using Arc so they can be passed to an async task
	content: Vec<Arc<dyn Element>>,
	/// Entry scope of the translation unit
	entry_scope: Rc<RefCell<Scope>>,
	/// Current scope of the translation unit
	current_scope: Rc<RefCell<Scope>>,
	/// Lsp data for this unit
	lsp: Option<RefCell<LangServerData>>,
	/// Reporting colors defined for this translation unit
	colors: ReportColors,
}

impl<'u> TranslationUnit<'u> {
	/// Creates a new translation unit
	///
	/// Should be called once for each distinct source file
	pub fn new(parser: &'u Parser, source: Arc<dyn Source>, with_lsp: bool, with_colors: bool) -> Self
	{
		let scope = Rc::new(RefCell::new(Scope::new(None, source, ParseMode::default())));
		Self {
			parser,
			source: source.clone(),
			content: vec![],
			entry_scope: scope.clone(),
			current_scope: scope,
			lsp: with_lsp.then(|| RefCell::new(LangServerData::default())),
			colors: with_colors.then(ReportColors::with_colors).unwrap_or(ReportColors::without_colors()),
		}
	}

	pub fn scope(&self) -> &Scope {
		&(*self.current_scope.clone()).borrow()
	}

	pub fn scope_mut(&self) -> &mut Scope {
		&mut (*self.current_scope.clone()).borrow_mut()
	}

	/// Runs procedure with a newly created scope from a source file
	pub fn with_child<F, R>(&mut self, source: Arc<dyn Source>, parse_mode: ParseMode, f: F) -> R
	where
		F: FnOnce(Rc<RefCell<Scope>>) -> R,
	{
		self.current_scope = self.current_scope.new_child(source, parse_mode);

		f(self.current_scope.clone())
	}

	/// Runs procedure with the language server's if language server processing is enabled
	pub fn with_lsp<F, R>(&self, f: F) -> Option<R>
		where F: FnOnce(RefMut<'_, LangServerData>) -> R
	{
		let Some(data) = &self.lsp else {
			return None;
		};

		Some(f(data.borrow_mut()))
	}

	pub fn consume(mut self) -> Self
	{
		self.parser.parse(&mut self);
		self
	}
}

pub trait TranslationAccessors {
	/// Adds content to the translation unit
	///
	/// Method [`Element::scoped`] is called to inform the element that it can be added to a scope
	fn add_content(&mut self, elem: Arc<dyn Element>) -> Result<(), Report>;
}

impl TranslationAccessors for TranslationUnit<'_> {
	fn add_content(&mut self, elem: Arc<dyn Element>) -> Result<(), Report>
	{
		elem.scoped(self.current_scope.clone())?;
		self.content.push(elem);
		Ok(())
	}
}
