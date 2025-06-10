use std::sync::Arc;

use regex::Captures;
use regex::Regex;

use crate::parser::rule::RegexRule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::Token;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

use super::elem::LineBreak;

#[auto_registry::auto_registry(registry = "rules")]
pub struct BreakRule {
	re: [Regex; 1],
}

impl Default for BreakRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r"(\n[^\S\r\n]*)+$").unwrap()],
		}
	}
}

impl RegexRule for BreakRule {
	fn name(&self) -> &'static str {
		"Break"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Meta
	}

	fn regexes(&self) -> &[regex::Regex] {
		&self.re
	}

	fn enabled(
		&self,
		_unit: &TranslationUnit,
		mode: &ParseMode,
		_states: &mut CustomStates,
		_index: usize,
	) -> bool {
		return !mode.paragraph_only;
	}

	fn on_regex_match<'u>(
		&self,
		_index: usize,
		unit: &mut TranslationUnit,
		token: Token,
		captures: Captures,
	) {
		let length = captures
			.get(1)
			.unwrap()
			.as_str()
			.chars()
			.fold(0usize, |count, c| count + (c == '\n') as usize);

		unit.add_content(Arc::new(LineBreak {
			location: token.clone(),
			length,
		}))
	}
}
