use std::rc::Rc;

use regex::Captures;
use regex::Regex;

use crate::parser::rule::RegexRule;
use crate::parser::source::Token;
use crate::parser::state::ParseMode;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

use super::elem::Latex;
use super::elem::TexKind;

#[auto_registry::auto_registry(registry = "rules")]
pub struct LatexRule {
	re: [Regex; 2],
}

impl Default for LatexRule {
	fn default() -> Self {
		Self {
			re: [
				Regex::new(r"\$\|(?:\[((?:\\.|[^\\\\])*?)\])?(?:((?:\\.|[^\\\\])*?)\|\$)?")
					.unwrap(),
				Regex::new(r"\$(?:\[((?:\\.|[^\\\\])*?)\])?(?:((?:\\.|[^\\\\])*?)\$)?").unwrap(),
			],
		}
	}
}

impl RegexRule for LatexRule {
	fn name(&self) -> &'static str {
		"Latex"
	}

	//FIXME: fn previous(&self) -> Option<&'static str> { Some("Comment") }
	fn previous(&self) -> Option<&'static str> {
		Some("Text") // TODO
	}

	fn regexes(&self) -> &[regex::Regex] {
		&self.re
	}

	fn enabled(&self, _mode: &ParseMode, _index: usize) -> bool {
		true
	}

	fn on_regex_match<'u>(
		&self,
		index: usize,
		unit: &mut TranslationUnit<'u>,
		token: Token,
		captures: Captures,
	) {
		unit.add_content(Rc::new(Latex {
			location: token,
			mathmode: false,
			kind: TexKind::Inline,
			env: "".into(),
			tex: "".into(),
		}));
	}
}
