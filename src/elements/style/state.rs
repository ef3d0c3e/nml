use std::fmt::Error;
use std::fmt::Formatter;
use std::sync::Arc;

use ariadne::Fmt;
use ariadne::Span;
use parking_lot::RwLock;
use regex::Regex;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::macros::*;
use crate::parser::reports::Report;
use crate::parser::reports::*;
use crate::parser::source::Token;
use crate::parser::state::CustomState;
use crate::unit::scope::Scope;
use crate::unit::translation::TranslationUnit;

pub struct Style {
	/// Style name
	pub(crate) name: String,
	/// Style enable regex
	pub(crate) start_re: Regex,
	/// Style disable regex
	pub(crate) end_re: Regex,
	/// Compile function
	pub(crate) compile: Arc<
		dyn Fn(bool, Arc<RwLock<Scope>>, &Compiler, &mut CompilerOutput) -> Result<(), Vec<Report>>
			+ Send
			+ Sync,
	>,
}

impl core::fmt::Debug for Style {
	fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
		#[derive(Debug)]
		struct Wrapper<'a> {
			name: &'a String,
			start_re: &'a Regex,
			end_re: &'a Regex,
		}

		core::fmt::Debug::fmt(
			&Wrapper {
				name: &self.name,
				start_re: &self.start_re,
				end_re: &self.end_re,
			},
			f,
		)
	}
}

pub static STYLE_STATE: &str = "nml.style.state";

/// State for styles
#[derive(Debug, Default)]
pub struct StyleState {
	/// Enabled styles and their enabled location
	pub(crate) enabled: Vec<(String, Token)>,
}

impl CustomState for StyleState {
	fn name(&self) -> &str {
		STYLE_STATE
	}

	fn on_scope_end(
		&mut self,
		unit: &mut TranslationUnit,
		scope: Arc<RwLock<Scope>>,
	) -> Vec<Report> {
		let mut reports = vec![];
		let scope_token: Token = scope.read().source().clone().into();

		self.enabled.iter().for_each(|(name, location)| {
			reports.push(make_err!(
				location.source(),
				"Unterminated style".into(),
				span(
					location.range.clone(),
					format!("Style {} starts here", name.fg(unit.colors().info))
				),
				span(
					scope_token.range.end()..scope_token.range.end(),
					"Scope ends here".into()
				)
			));
		});

		reports
	}
}
