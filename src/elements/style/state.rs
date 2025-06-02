use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;

use ariadne::Fmt;
use ariadne::Span;
use regex::Regex;

use crate::parser::reports::macros::*;
use crate::parser::reports::Report;
use crate::parser::reports::*;
use crate::parser::source::Token;
use crate::parser::state::CustomState;
use crate::report_err;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::CustomData;
use crate::unit::translation::TranslationUnit;

#[derive(Debug)]
pub struct Style {
	/// Style name
	pub(crate) name: String,
	/// Style enable regex
	pub(crate) start_re: Regex,
	/// Style disable regex
	pub(crate) end_re: Regex,
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

	fn on_scope_end(&self, unit: &mut TranslationUnit, scope: Rc<RefCell<Scope>>) -> Vec<Report> {
		let mut reports = vec![];
		let scope_token: Token = scope.borrow().source().clone().into();

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

pub static STYLE_CUSTOM: &str = "nml.style.registered";
/// Data for styles
pub struct StyleData {
	/// All registered styles
	pub(crate) registered: Vec<Rc<Style>>,
}

impl CustomData for StyleData {
	fn name(&self) -> &str {
		STYLE_CUSTOM
	}
}
