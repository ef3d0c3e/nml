use std::sync::Arc;

use parking_lot::RwLock;
use regex::Regex;

use crate::elements::layout::custom::LayoutData;
use crate::elements::layout::custom::LAYOUT_CUSTOM;
use crate::parser::rule::RegexRule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::Token;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::unit::translation::CustomData;
use crate::unit::translation::TranslationUnit;

#[auto_registry::auto_registry(registry = "rules")]
pub struct LayoutRule {
	re: [Regex; 1],
}

impl Default for LayoutRule {
	fn default() -> Self {
		Self {
			re: [
				Regex::new(
					r"(?:^|\n):layout(?:[^\S\r\n]+(\w+)(?:[^\S\r\n]+(.*))?)?",
				)
				.unwrap(),
			],
		}
	}
}

impl RegexRule for LayoutRule {
	fn name(&self) -> &'static str {
		"Layout"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Command
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
		!mode.paragraph_only
	}

	fn on_regex_match<'u>(
		&self,
		_index: usize,
		unit: &mut TranslationUnit,
		token: Token,
		captures: regex::Captures,
	) {
		if !unit.has_data(LAYOUT_CUSTOM) {
			unit.new_data(Arc::new(RwLock::new(LayoutData::default())));
		}

		let data = unit.get_data(LAYOUT_CUSTOM);
		let mut lock = data.write();
		let data = lock.downcast_mut::<LayoutData>().unwrap();
	}
}
