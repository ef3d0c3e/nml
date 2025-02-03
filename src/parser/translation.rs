use core::slice::SlicePattern;
use std::borrow::BorrowMut;
use std::cell::Ref;
use std::cell::RefCell;
use std::cell::RefMut;
use std::rc::Rc;
use std::sync::Arc;

use crate::document::element::Element;
use crate::elements::block::data::BlockHolder;
use crate::elements::customstyle::custom::CustomStyleHolder;
use crate::elements::layout::data::LayoutHolder;
use crate::lsp::data::LangServerData;
use crate::lua::kernel::Kernel;
use crate::lua::kernel::KernelHolder;

use super::new::Parser;
use super::parser::ReportColors;
use super::reports::Report;
use super::scope::Scope;
use super::scope::ScopeAccessor;
use super::source::Source;
use super::state::ParseMode;
use super::style::StyleHolder;

/// Stores the data required by the parser
pub struct TranslationUnit<'u> {
	/// Parser for this translation unit
	pub parser: &'u Parser,
	/// Entry point of this translation unit
	source: Arc<dyn Source>,
	/// Reporting colors defined for this translation unit
	colors: ReportColors,
	/// Resulting AST
	/// Elements are stored using Arc so they can be passed to an async task
	content: Vec<(Rc<RefCell<Scope>>, Arc<dyn Element>)>,
	/// Entry scope of the translation unit
	entry_scope: Rc<RefCell<Scope>>,
	/// Current scope of the translation unit
	current_scope: Rc<RefCell<Scope>>,
	/// Lsp data for this unit (shared with children scopes)
	lsp: Option<RefCell<LangServerData>>,

	/// Available kernels for this translation unit
	lua_kernels: KernelHolder,
	/// Available layouts
	layouts: LayoutHolder,
	/// Available blocks
	blocks: BlockHolder,
	/// Custom element styles
	elem_styles: StyleHolder,
	/// User-defined styles
	custom_styles: CustomStyleHolder,

	reports: Vec<Report>,
}

///
/// # Scope
///
/// The translation unit will manage scope accordingly to specific objects
///
/// ## References and Variables
///
/// The lifetime and scoping of variables and references follow the same set of rules:
///  * When defined, they will overwrite any previously defined variable or reference (in the same scope) using the same key.
///  * Calling a variable or reference will result in a recursive search in the current scope and all parent of that scope.
/// Whenever a variable is defined, it will overwrite the previously defined variable, if they have the same key in the current scope.
///
impl<'u> TranslationUnit<'u> {
	/// Creates a new translation unit
	///
	/// Should be called once for each distinct source file
	pub fn new(
		parser: &'u Parser,
		source: Arc<dyn Source>,
		with_lsp: bool,
		with_colors: bool,
	) -> Self {
		let scope = Rc::new(RefCell::new(Scope::new(
			None,
			source.clone(),
			ParseMode::default(),
			0,
		)));
		let mut s = Self {
			parser,
			source: source,
			colors: with_colors
				.then(ReportColors::with_colors)
				.unwrap_or(ReportColors::without_colors()),
			content: vec![],
			entry_scope: scope.clone(),
			current_scope: scope,
			lsp: with_lsp.then(|| RefCell::new(LangServerData::default())),

			lua_kernels: KernelHolder::new(parser),
			layouts: LayoutHolder::default(),
			blocks: BlockHolder::default(),
			elem_styles: StyleHolder::default(),
			custom_styles: CustomStyleHolder::default(),

			reports: Vec::default(),
		};

		s.lua_kernels
			.insert("main".to_string(), Kernel::new(parser));
		s
	}

	pub fn scope<'s>(&'s self) -> Ref<'s, Scope> { (*self.current_scope).borrow() }

	pub fn scope_mut<'s>(&'s self) -> RefMut<'s, Scope> { (*self.current_scope).borrow_mut() }

	/// Runs procedure with a newly created scope from a source file
	pub fn with_child<F, R>(&mut self, source: Arc<dyn Source>, parse_mode: ParseMode, f: F) -> R
	where
		F: FnOnce(Rc<RefCell<Scope>>) -> R,
	{
		let prev_scope = self.current_scope.clone();

		self.current_scope = prev_scope.new_child(source, parse_mode);
		let ret = f(self.current_scope.clone());
		self.current_scope = prev_scope;

		ret
	}

	/// Runs procedure with the language server, if language server processing is enabled
	pub fn with_lsp<F, R>(&self, f: F) -> Option<R>
	where
		F: FnOnce(RefMut<'_, LangServerData>) -> R,
	{
		self.lsp.as_ref().map(|data| f(data.borrow_mut()))
	}

	/// Consumes the translation unit with it's current scope
	pub fn consume(mut self) -> Self {
		self.parser.parse(&mut self);
		if let Some(lsp) = &mut self.lsp {
			// TODO: send to lsp
		} else {
			Report::reports_to_stdout(&self.colors, std::mem::replace(&mut self.reports, vec![]));
		}

		self
	}

	pub fn add_report(&mut self, report: Report) {
		self.reports.push(report);
	}

	pub fn colors(&self) -> &ReportColors {
		&self.colors
	}
}

pub trait TranslationAccessors {
	/// Adds content to the translation unit
	fn add_content(&mut self, elem: Arc<dyn Element>);

	/// Adds a new report to this translation unit
	fn report(&mut self, report: Report);

	/// Gets the content associated with a scope
	fn content(&self, scope: Rc<RefCell<Scope>>) -> &[(Rc<RefCell<Scope>>, Arc<dyn Element>)];
}

impl TranslationAccessors for TranslationUnit<'_> {
	fn add_content(&mut self, elem: Arc<dyn Element>) {
		self.current_scope.add_content();
		self.content.push((self.current_scope.clone(), elem));
	}

	fn report(&mut self, report: Report) { self.reports.push(report); }

	fn content(&self, scope: Rc<RefCell<Scope>>) -> &[(Rc<RefCell<Scope>>, Arc<dyn Element>)]
	{
		let range = scope.borrow().range.clone();
		&(self.content).as_slice()[range]
	}
}
