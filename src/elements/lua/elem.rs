use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;

use crate::elements::meta::scope::ScopeElement;
use crate::elements::text::elem::Text;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use parking_lot::RwLock;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::lua::kernel::KernelContext;
use crate::lua::kernel::KernelName;
use crate::parser::reports::Report;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use crate::parser::state::ParseMode;
use crate::unit::element::ContainerElement;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

use super::custom::LuaData;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LuaEvalKind {
	/// Discard evaluation result
	None,
	/// Evaluates to string, output it as text
	String,
	/// Evaluates to string, then parse it
	StringParse,
}

impl FromStr for LuaEvalKind {
	type Err = &'static str;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"" => Ok(LuaEvalKind::None),
			"'" => Ok(LuaEvalKind::String),
			"!" => Ok(LuaEvalKind::StringParse),
			_ => Err("Invalid evaluation kind"),
		}
	}
}

#[derive(Debug)]
pub struct LuaPostProcess {
	pub(crate) location: Token,
	/// Expanded content after post processing
	pub(crate) expanded: OnceLock<Vec<Arc<RwLock<Scope>>>>,
	/// Lua content source
	pub(crate) source: Arc<dyn Source>,
	/// Lua kernel nam
	pub(crate) kernel_name: KernelName,
	/// Kind of evaluation required
	pub(crate) eval_kind: LuaEvalKind,
}

impl LuaPostProcess {
	/// Run post-processing tasks for this element
	pub fn process<'ctx>(&self, unit: &mut TranslationUnit) {
		LuaData::initialize(unit);
		LuaData::with_kernel(unit, &self.kernel_name, |unit, kernel| {
			let parsed = unit.with_child(
				self.source.clone(),
				ParseMode::default(),
				true,
				|unit, scope| {
					let ctx = KernelContext::new(self.source.clone().into(), unit);
					match kernel.run_with_context(ctx, |lua| match self.eval_kind {
						LuaEvalKind::None => lua
							.load(self.source.content())
							.set_name(self.source.name())
							.eval::<()>()
							.map(|_| String::default()),
						LuaEvalKind::String | LuaEvalKind::StringParse => lua
							.load(self.source.content())
							.set_name(self.source.name())
							.eval::<String>(),
						_ => panic!(),
					}) {
						Err(err) => {
							report_err!(
								unit,
								self.location().source(),
								"Lua Error".into(),
								span(self.location().range.clone(), err.to_string())
							);
						}
						Ok(result) => {
							if self.eval_kind == LuaEvalKind::String && !result.is_empty() {
								unit.add_content(Arc::new(Text {
									location: self.source.clone().into(),
									content: result,
								}));
							} else if self.eval_kind == LuaEvalKind::StringParse
								&& !result.is_empty()
							{
								let content = Arc::new(VirtualSource::new(
									self.location().clone(),
									":LUA:Inline lua result".into(),
									result,
								));
								let mode = unit.get_scope().read().parser_state().mode.clone();
								let scope = unit.with_child(
									content as Arc<dyn Source>,
									mode,
									true,
									|unit, scope| {
										unit.parser.clone().parse(unit);
										scope
									},
								);
								unit.add_content(Arc::new(ScopeElement {
									token: self.source.clone().into(),
									scope: [scope],
								}));
							}
						}
					}
					scope
				},
			);
			self.expanded
				.set(vec![parsed])
				.expect("Duplicate post-processing task");
		});
	}
}

impl Element for LuaPostProcess {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Compound
	}

	fn element_name(&self) -> &'static str {
		"Lua Post-Process"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		let content = self.expanded.get().expect("Expected post-processing");

		for (scope, elem) in (&content[0]).content_iter(false) {
			elem.compile(scope, compiler, output)?;
		}
		Ok(())
	}
}

impl ContainerElement for LuaPostProcess {
	fn contained(&self) -> &[Arc<RwLock<Scope>>] {
		match self.expanded.get() {
			Some(content) => content.as_slice(),
			_ => &[],
		}
	}
}
