use std::any::Any;
use std::ops::Range;
use std::sync::Arc;

use ariadne::Fmt;
use ariadne::Span;
use parking_lot::RwLock;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::elements::layout::elem::LayoutToken;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::source::Token;
use crate::parser::state::CustomState;
use crate::unit::scope::Scope;
use crate::unit::translation::TranslationUnit;

pub trait Layout: core::fmt::Debug {
	fn name(&self) -> &str;
	fn expects(&self) -> Range<usize>;
	fn parse_properties(
		&self,
		unit: &mut TranslationUnit,
		token: Token,
	) -> Option<Box<dyn Any + Send + Sync>>;
	fn compile(
		&self,
		scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
		id: usize,
		token: LayoutToken,
		params: &Option<Box<dyn Any + Send + Sync>>,
	) -> Result<(), Vec<Report>>;
}

pub static LAYOUT_STATE: &str = "nml.layout.state";

#[derive(Debug, Default)]
pub struct LayoutState {
	pub(crate) state: Vec<(Arc<dyn Layout + Send + Sync>, Token, usize)>,
}

impl CustomState for LayoutState {
	fn name(&self) -> &str {
		LAYOUT_STATE
	}

	fn on_scope_end(
		&mut self,
		_unit: &mut TranslationUnit,
		_scope: Arc<RwLock<Scope>>,
	) -> Vec<Report> {
		vec![]
	}

	fn on_document_end(
		&mut self,
		unit: &mut TranslationUnit,
		scope: Arc<RwLock<Scope>>,
	) -> Vec<Report> {
		let mut reports = vec![];
		let scope_token: Token = scope.read().source().clone().into();

		self.state.iter().for_each(|(layout, location, _id)| {
			reports.push(make_err!(
				location.source(),
				"Unterminated Layout".into(),
				span(
					location.range.clone(),
					format!(
						"Layout {} starts here",
						layout.name().fg(unit.colors().info)
					)
				),
				span(
					scope_token.range.end()..scope_token.range.end(),
					"Docment ends here".into()
				),
				help(format!(
					"Insert `{}` before the document end",
					":layout end".fg(unit.colors().highlight)
				))
			));
		});

		reports
	}
}
