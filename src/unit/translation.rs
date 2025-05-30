use std::cell::OnceCell;
use std::cell::RefCell;
use std::cell::RefMut;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;
use std::sync::Arc;

use graphviz_rust::print;
use rusqlite::params;
use rusqlite::Connection;
use tokio::sync::MutexGuard;

use crate::cache::cache::Cache;
use crate::lsp::data::LangServerData;
use crate::lua::kernel::Kernel;
use crate::lua::kernel::KernelHolder;
use crate::parser::parser::Parser;
use crate::parser::reports::Report;
use crate::parser::reports::ReportColors;
use crate::parser::reports::ReportKind;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::state::ParseMode;
use crate::util::settings::ProjectOutput;
use crate::util::settings::ProjectSettings;

use super::element::Element;
use super::element::LinkableElement;
use super::element::ReferenceableElement;
use super::scope::Scope;
use super::scope::ScopeAccessor;
use super::variable::PropertyValue;
use super::variable::PropertyVariable;
use super::variable::VariableMutability;
use super::variable::VariableName;
use super::variable::VariableVisibility;

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

	/// Error reports
	reports: Vec<(Rc<RefCell<Scope>>, Report)>,
	
	/// Path relative to the database
	path: String,
	/// Exported (internal) references
	references: HashMap<String, Rc<dyn ReferenceableElement>>,
	/// Output data extracted from parsing
	output: OnceCell<UnitOutput>,

	/// Per unit project settings
	settings: OnceCell<ProjectSettings>,
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
		path: String,
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
			source,
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

			path,
			reports: Vec::default(),
			references: HashMap::default(),
			output: OnceCell::default(),

			settings: OnceCell::default(),
		};

		let main_kernel = Kernel::new(&s);
		s.lua_kernels.insert("main".into(), main_kernel);
		s
	}

	pub fn token(&self) -> Token {
		self.source.clone().into()
	}

	pub fn parser(&self) -> &'u Parser { &self.parser }

	/// Gets the current scope
	pub fn get_scope(&self) -> &Rc<RefCell<Scope>> { &self.current_scope }

	/// Gets the entry scope
	pub fn get_entry_scope(&self) -> &Rc<RefCell<Scope>> { &self.entry_scope }

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
	/// Returns `None` if an error happened
	pub fn consume(mut self, output_file: String) -> Option<Self> {
		// Insert default variables
		let token = Token::new(0..0, self.source.clone());
		self.get_entry_scope()
			.insert_variable(Rc::new(
				PropertyVariable {
					location: token.clone(),
					name: VariableName::try_from("nml.input_file").unwrap(),
					visibility: VariableVisibility::Internal,
					mutability: VariableMutability::Immutable,
					value: PropertyValue::String(self.source.name().into()),
					value_token: token.clone(),
				}));
		self.get_entry_scope()
			.insert_variable(Rc::new(
				PropertyVariable {
					location: token.clone(),
					name: VariableName::try_from("nml.output_file").unwrap(),
					visibility: VariableVisibility::Internal,
					mutability: VariableMutability::Mutable,
					value: PropertyValue::String(output_file),
					value_token: token.clone(),
				}));
		self.get_entry_scope()
			.insert_variable(Rc::new(
				PropertyVariable {
					location: token.clone(),
					name: VariableName::try_from("nml.reference_key").unwrap(),
					visibility: VariableVisibility::Internal,
					mutability: VariableMutability::Mutable,
					value: PropertyValue::String(self.path.to_string()),
					value_token: token.clone(),
				}));

		self.parser.parse(&mut self);
		if let Some(lsp) = &mut self.lsp {
			// TODO: send to lsp
		} else {
			let reports = self.reports.drain(..).map(|(_, report)| report).collect::<Vec<_>>();
			let has_error = reports.iter().any(|report| report.kind == ReportKind::Error);
			Report::reports_to_stdout(&self.colors, reports);
			if has_error
			{
				return None;
			}
		}

		let output_file = self.get_scope().get_variable(&VariableName("nml.output_file".into()));
		let output = UnitOutput {
			input_file: self.path.clone(),
			output_file: output_file.map(|(var, _)| var.to_string()),
		};
		self.output.set(output).unwrap();

		Some(self)
	}
	pub fn colors<'s>(&'s self) -> &'s ReportColors {
		&self.colors
	}

	pub fn report(&mut self, report: Report) { self.reports.push((self.current_scope.clone(), report)); }

	/// Returns the path of the unit relative to the project's root. This is used to uniquely identify each units.
	pub fn input_path(&self) -> &String {
		&self.path
	}
	
	/// Gets the output path for this unit
	pub fn output_path(&self) -> Option<&String>
	{
		self.output.get()
			.map(|out| out.output_file.as_ref().unwrap())
	}

	pub fn reference_key(&self) -> String {
		let varname = VariableName::try_from("nml.reference_key").unwrap();
		self.get_scope()
			.get_variable(&varname)
			.map(|(var, _)| var.to_string())
			.unwrap()
	}

	/// Export all references of this [`TranslationUnit`]
	pub fn export_references(&self, cache: Arc<Cache>) -> Result<(), String>
	{
		let output = self.output.get().unwrap();

		cache.export_ref_unit(&self, &output.input_file, &output.output_file);
		cache.export_references(&self.reference_key(), self.references.iter())
	}
}

pub trait TranslationAccessors {
	/// Adds content to the translation unit's current scope
	fn add_content(&mut self, elem: Rc<dyn Element>);

	/// Adds a reference, note that this is not necessary to call
	fn add_reference(&mut self, elem: Rc<dyn ReferenceableElement>);

	/// Finds an internal reference, with name `name`, declared in this document
	fn get_reference<S: AsRef<str>>(&self, refname: S) -> Option<Rc<dyn ReferenceableElement>>;

	/// Returns the hashmap containing all referenceables in this unit
	fn references(&self) -> &HashMap<String, Rc<dyn ReferenceableElement>>;

	/// Update unit project setting
	fn update_settings(&self, settings: ProjectSettings);

	/// Gets the unit's settings (will panic if not set)
	fn get_settings(&self) -> &ProjectSettings;
}

impl TranslationAccessors for TranslationUnit<'_> {
	fn add_content(&mut self, elem: Rc<dyn Element>) {
		if let Some(reference) = elem.clone().as_referenceable()
		{
			self.add_reference(reference);
		}
		self.current_scope.add_content(elem);
	}

	fn add_reference(&mut self, elem: Rc<dyn ReferenceableElement>)
	{
		self.references.insert(elem.reference().name().to_string(), elem);
	}

	fn get_reference<S: AsRef<str>>(&self, name: S) -> Option<Rc<dyn ReferenceableElement>>
	{
		self.references.get(name.as_ref()).cloned()
	}

	fn references(&self) -> &HashMap<String, Rc<dyn ReferenceableElement>>
	{
		&self.references
	}

	fn update_settings(&self, mut settings: ProjectSettings)
	{
		let scope = self.get_scope();

		match &mut settings.output {
			ProjectOutput::Html(html) => {
				if let Some((var, _)) =
					scope.get_variable(&VariableName("html.language".to_string()))
				{
					html.language = var.to_string();
				}
				if let Some((var, _)) = scope.get_variable(&VariableName("html.icon".to_string())) {
					html.icon = Some(var.to_string())
				}
				if let Some((var, _)) = scope.get_variable(&VariableName("html.css".to_string())) {
					html.icon = Some(var.to_string());
				}
			}
		}
		self.settings.set(settings).unwrap();
	}

	fn get_settings(&self) -> &ProjectSettings {
		self.settings.get().unwrap()
	}
}
