use std::cell::OnceCell;
use std::cell::RefCell;
use std::cell::RefMut;
use std::rc::Rc;
use std::sync::Arc;

use crate::document::element::Element;
use crate::document::variable::VariableName;
use crate::lsp::data::LangServerData;
use crate::lua::kernel::Kernel;
use crate::lua::kernel::KernelHolder;

use super::parser::Parser;
use super::reports::Report;
use super::reports::ReportColors;
use super::scope::Scope;
use super::scope::ScopeAccessor;
use super::source::Source;
use super::source::SourceFile;
use super::state::ParseMode;

/// Stores output data for [`TranslationUnit`]
#[derive(Debug)]
pub struct UnitOutput
{
	pub input_file: String,
	pub output_file: Option<String>,
}

/// Stores the data required by the parser
pub struct TranslationUnit<'u> {
	/// Parser for this translation unit
	pub parser: &'u Parser,
	/// Entry point of this translation unit
	source: Arc<dyn Source>,
	/// Reporting colors defined for this translation unit
	colors: ReportColors,
	/// Entry scope of the translation unit
	entry_scope: Rc<RefCell<Scope>>,
	/// Current scope of the translation unit
	current_scope: Rc<RefCell<Scope>>,
	/// Lsp data for this unit (shared with children scopes)
	lsp: Option<RefCell<LangServerData>>,

	/// Available kernels for this translation unit
	lua_kernels: KernelHolder,
	/// Available layouts
	//layouts: LayoutHolder,
	/// Available blocks
	//blocks: BlockHolder,
	/// Custom element styles
	//elem_styles: StyleHolder,
	/// User-defined styles
	//custom_styles: CustomStyleHolder,

	reports: Vec<(Rc<RefCell<Scope>>, Report)>,
	/// Output data extracted from parsing
	output: OnceCell<UnitOutput>,
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
			entry_scope: scope.clone(),
			current_scope: scope,
			lsp: with_lsp.then(|| RefCell::new(LangServerData::default())),

			lua_kernels: KernelHolder::default(),
			//layouts: LayoutHolder::default(),
			//blocks: BlockHolder::default(),
			//elem_styles: StyleHolder::default(),
			//custom_styles: CustomStyleHolder::default(),

			reports: Vec::default(),
			output: OnceCell::default(),
		};

		let main_kernel = Kernel::new(&s);
		s.lua_kernels.insert("main".into(), main_kernel);
		s
	}

	pub fn parser(&self) -> &'u Parser { &self.parser }

	/// Gets the current scope
	pub fn get_scope(&self) -> &Rc<RefCell<Scope>> { &self.current_scope }

	/// Gets the entry scope
	pub fn get_entry_scope(&self) -> &Rc<RefCell<Scope>> { &self.entry_scope }

	//pub fn scope<'s>(&'s self) -> Ref<'s, Scope> { (*self.current_scope).borrow() }

	//pub fn scope_mut<'s>(&'s self) -> RefMut<'s, Scope> { (*self.current_scope).borrow_mut() }

	/// Runs procedure with a newly created scope from a source file
	///
	/// # Parameters
	///
	/// - `source` is the source (usually a [`VirtualSource`]) that holds the content
	/// - `parse_mode` is used to specify a custom parsing mode for the children scope
	/// - `paragraphing` controls whether paragraphing is enabled for the child scope
	pub fn with_child<F, R>(&mut self, source: Arc<dyn Source>, parse_mode: ParseMode, paragraphing: bool, f: F) -> R
	where
		F: FnOnce(&mut TranslationUnit<'u>, Rc<RefCell<Scope>>) -> R,
	{
		let prev_scope = self.current_scope.clone();

		self.current_scope = prev_scope.new_child(source, parse_mode, paragraphing);
		let ret = f(self, self.current_scope.clone());
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

		let input_file = Rc::as_ref(self.get_entry_scope()).borrow().source();

		let output_file = self.get_scope().get_variable(&VariableName("compiler.output".into()));
		let output = UnitOutput {
			input_file: input_file.downcast_ref::<SourceFile>().unwrap().name().to_string(),
			output_file: output_file.map(|(var, _)| "TODO".to_string()),
		};
		self.output.set(output).unwrap();

		self
	}
	pub fn colors<'s>(&'s self) -> &'s ReportColors {
		&self.colors
	}

	pub fn report(&mut self, report: Report) { self.reports.push((self.current_scope.clone(), report)); }
}

pub trait TranslationAccessors {
	/// Adds content to the translation unit's current scope
	fn add_content(&mut self, elem: Rc<dyn Element>);
}

impl TranslationAccessors for TranslationUnit<'_> {
	fn add_content(&mut self, elem: Rc<dyn Element>) {
		self.current_scope.add_content(elem);
	}
}
