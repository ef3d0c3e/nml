use std::sync::Arc;

use regex::Captures;
use regex::Regex;

use crate::elements::comment::elem::Comment;
use crate::parser::rule::RegexRule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::Token;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

#[auto_registry::auto_registry(registry = "rules")]
pub struct CommentRule {
	re: [Regex; 1],
}

impl Default for CommentRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r"(?:^|\s)::(.*)").unwrap()],
		}
	}
}

impl RegexRule for CommentRule {
	fn name(&self) -> &'static str {
		"Comment"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Inline
	}

	fn regexes(&self) -> &[regex::Regex] {
		&self.re
	}

	fn enabled(
		&self,
		_unit: &TranslationUnit,
		_mode: &ParseMode,
		_states: &mut CustomStates,
		_index: usize,
	) -> bool {
		true
	}

	fn on_regex_match<'u>(
		&self,
		_index: usize,
		unit: &mut TranslationUnit,
		token: Token,
		captures: Captures,
	) {
		let content = captures.get(1).unwrap();
		unit.with_lsp(|lsp| lsp.with_semantics(token.source(), |sems, tokens| {
			sems.add(token.start()+1..content.end(), tokens.comment);
		}));

		unit.add_content(Arc::new(Comment {
			location: token,
			content: content.as_str().to_string(),
		}));
	}
}
