use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;

use crate::lua::wrappers::*;
use crate::parser::parser::Parser;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::source::Source;
use crate::parser::source::SourceFile;
use crate::unit::translation::TranslationUnit;
use crate::unit::variable::PropertyVariable;
use crate::unit::variable::VariableName;
use ariadne::Span;
use auto_userdata::auto_userdata;
use mlua::AnyUserData;
use mlua::Lua;
use parking_lot::RwLock;
use rusqlite::Transaction;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ContainerElement;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;

#[derive(Debug)]
#[auto_userdata(proxy = "ImportProxy", immutable, mutable)]
pub struct Import {
	#[lua_ud]
	pub(crate) location: Token,
	#[lua_proxy(VecScopeProxy)]
	pub(crate) content: Vec<Arc<RwLock<Scope>>>,
}

impl Element for Import {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Compound
	}

	fn element_name(&self) -> &'static str {
		"Import"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		for scope in self.content.iter() {
			for (scope, elem) in scope.content_iter(false) {
				elem.compile(scope, compiler, output)?
			}
		}
		Ok(())
	}

	fn provide_hover(&self) -> Option<String> {
		Some(format!(
			"Import

# Properties
 * **Location**: [{}] ({}..{})",
			self.location.source().name().display(),
			self.location().range.start(),
			self.location().range.end(),
		))
	}

	fn as_container(self: Arc<Self>) -> Option<Arc<dyn ContainerElement>> {
		Some(self)
	}

	fn lua_ud(&self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(ImportProxy(self as *const _)).unwrap()
	}

	fn lua_ud_mut(&mut self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(ImportProxyMut(self as *mut _)).unwrap()
	}
}

impl ContainerElement for Import {
	fn contained(&self) -> &[Arc<RwLock<Scope>>] {
		self.content.as_slice()
	}
}

#[derive(Debug)]
#[auto_userdata(proxy = "LazyImportProxy", immutable, mutable)]
pub struct LazyImport {
	#[lua_ud]
	pub(crate) location: Token,
	pub(crate) path: PathBuf,
	pub(crate) output: PathBuf,
	#[lua_ignore]
	pub(crate) source: Arc<dyn Source>,
	#[lua_ignore]
	pub(crate) expanded: OnceLock<Vec<Arc<RwLock<Scope>>>>,
}

impl LazyImport {
	pub fn process(&self, parser: Arc<Parser>) -> Result<(), Vec<Report>>
	{
		let tu = TranslationUnit::new(self.path.clone(), parser, self.source.clone(), false, true);
		let (reports, unit) = tu.consume(self.output.clone());
		let is_meta = unit.get_scope().get_variable(&VariableName("nml.meta".into()))
			.map_or(false, |(var, _)| {
				let Some(var) = var.downcast_ref::<PropertyVariable>() else { return false };
				match var.value {
					crate::unit::variable::PropertyValue::Integer(1) => true,
					_ => false
				}
			});
		if !is_meta {
			panic!("Cannot import non-meta unit");
			// TODO
		}
		if !reports.is_empty() {
			return Err(reports);
		}
		self.expanded.set(
			vec![unit.get_entry_scope().clone()]
		).unwrap();
		// warn if not meta
		Ok(())
	}
}

impl Element for LazyImport {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Compound
	}

	fn element_name(&self) -> &'static str {
		"Lazy Import"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		let content = self.expanded.get().expect("Expected lazy import processing");

		for (scope, elem) in content[0].content_iter(false)
		{
			elem.compile(scope, compiler, output)?;
		}
		Ok(())
	}

	fn provide_hover(&self) -> Option<String> {
		Some(format!(
			"Lazy Import

# Properties"
		))
	}

	fn lua_ud(&self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(LazyImportProxy(self as *const _))
			.unwrap()
	}

	fn lua_ud_mut(&mut self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(LazyImportProxyMut(self as *mut _))
			.unwrap()
	}
}
