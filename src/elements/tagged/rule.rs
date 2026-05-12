use std::{any::Any, ops::Range};

use crate::parser::reports::macros::*;
use crate::parser::reports::*;

use ariadne::Fmt;
use regex::Regex;

use crate::{
	parser::{
		rule::{Rule, RuleTarget},
		source::Cursor,
		state::{CustomStates, ParseMode},
	},
	unit::translation::TranslationUnit,
};

#[auto_registry::auto_registry(registry = "rules")]
pub struct TaggedRule {
	start_re: Regex,
}

impl Default for TaggedRule {
	fn default() -> Self {
		Self {
			start_re: Regex::new(r#"\{@(\w+)"#).unwrap(),
		}
	}
}

impl Rule for TaggedRule {
	fn name(&self) -> &'static str {
		"Tagged"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Inline
	}

	fn next_match(
		&self,
		_unit: &TranslationUnit,
		_mode: &ParseMode,
		_states: &mut CustomStates,
		cursor: &Cursor,
	) -> Option<(Range<usize>, Box<dyn Any + Send + Sync>)> {
		self.start_re
			.find_at(cursor.source().content(), cursor.pos())
			.map(|m| {
				(
					m.range(),
					Box::new([false; 0]) as Box<dyn Any + Send + Sync>,
				)
			})
	}

	fn on_match<'u>(
		&self,
		unit: &mut TranslationUnit,
		cursor: &Cursor,
		_match_data: Box<dyn Any + Send + Sync>,
	) -> Cursor {
		let source = cursor.source();
		let content = source.content();
		let captures = self.start_re.captures_at(content, cursor.pos()).unwrap();
		assert_eq!(captures.get(0).unwrap().start(), cursor.pos());

		let tag = captures.get(1).unwrap();
		unit.with_lsp(|lsp| {
			lsp.with_semantics(cursor.source(), |sems, tokens| {
				// {@
				sems.add(
					captures.get(0).unwrap().start()..captures.get(0).unwrap().start() + 2,
					tokens.tagged_delim,
				);
				// tag
				sems.add(tag.range(), tokens.tagged_tag);
			})
		});

		let mut delims = vec![];
		let start = captures.get(0).unwrap().end();
		let mut last = start;
		let mut balance = 1;
		for (i, ch) in content[start..].char_indices() {
			if ch == '{' {
				if balance == 1 {
					last = start + i + 1;
				}
				balance += 1;
			} else if ch == '}' {
				balance -= 1;
				if balance == 1 {
					delims.push(last..start + i);
				}
				if balance == 0 {
					last = start + i + 1;
					break;
				}
			}
		}
		if balance != 0 {
			report_err!(
				unit,
				cursor.source(),
				"Invalid Tagged Content".into(),
				span(
					captures.get(0).unwrap().start()..last,
					format!("Unmatched `{}`", "{".fg(unit.colors().highlight))
				)
			);
			return cursor.at(last);
		}
		// If empty, insert entire range
		if delims.is_empty() {
			delims.push(start..last - 1);
		}
		// Trim
		for range in delims.iter_mut() {
			while b" \t\n".contains(&content.as_bytes()[range.start]) {
				range.start += 1;
			}
			while b" \t\n".contains(&content.as_bytes()[range.end-1]) {
				range.end -= 1;
			}
		}
		for (i, range) in delims.iter().enumerate() {
			println!("{i}: `{}'", &content[range.clone()]);
		}
		cursor.at(last)
	}
}
