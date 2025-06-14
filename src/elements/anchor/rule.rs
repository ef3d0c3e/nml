use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::RuleTarget;
use crate::parser::source::SourcePosition;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::unit::references::InternalReference;
use crate::unit::references::Refname;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;
use ariadne::Fmt;
use regex::Captures;
use regex::Regex;
use std::sync::Arc;
use std::sync::OnceLock;

use crate::parser::reports::Report;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;

use super::completion::AnchorCompletion;
use super::elem::Anchor;

#[auto_registry::auto_registry(registry = "rules")]
pub struct AnchorRule {
	re: [Regex; 1],
}

impl Default for AnchorRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r"(:anchor)[^\S\r\n]+([^:\r\n]*)?(:)?").unwrap()],
		}
	}
}

impl RegexRule for AnchorRule {
	fn name(&self) -> &'static str {
		"Anchor"
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
		_id: usize,
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
		let anchor = captures.get(2).unwrap();

		// Missing ':'
		if captures.get(3).is_none() {
			report_err!(
				unit,
				token.source(),
				"Invalid anchor".into(),
				span(
					token.end()..anchor.end() + 1,
					format!("Missing closing `{}`", ":".fg(unit.colors().info))
				),
				span_highlight(
					token.start()..token.start() + 1,
					format!("Opening `{}` here", ":".fg(unit.colors().highlight))
				),
				note("While attempting to parse anchor".into())
			);
			return;
		}

		// Parse to refname
		let anchor_refname = match Refname::try_from(anchor.as_str()) {
			// Parse error
			Err(err) => {
				report_err!(
					unit,
					token.source(),
					"Invalid anchor".into(),
					span(anchor.range(), err),
					note("While attempting to parse anchor".into())
				);
				return;
			}
			// Check format
			Ok(r) => match r {
				Refname::Internal(_) => r,
				_ => {
					report_err!(
						unit,
						token.source(),
						"Invalid anchor".into(),
						span(
							anchor.range(),
							format!("Use of reserved character: `{}` (external reference), `{}` (bibliography)", '#'.fg(unit.colors().info), '@'.fg(unit.colors().info))
						),
						note("While attempting to parse anchor".into())
					);
					return;
				}
			},
		};

		unit.with_lsp(|lsp| {
			lsp.with_semantics(token.source(), |sems, tokens| {
				sems.add(captures.get(1).unwrap().range(), tokens.command);
				sems.add(anchor.range(), tokens.anchor_refname);
				sems.add(captures.get(3).unwrap().range(), tokens.command);
			})
		});

		let reference = Arc::new(InternalReference::new(
			token.source().original_range(token.range.clone()),
			anchor_refname.clone(),
		));
		unit.add_content(Arc::new(Anchor {
			location: token.clone(),
			refname: anchor_refname.clone(),
			reference: reference.clone(),
			link: OnceLock::default(),
		}));
	}

	fn completion(
		&self,
	) -> Option<Box<dyn lsp::completion::CompletionProvider + 'static + Send + Sync>> {
		Some(Box::new(AnchorCompletion {}))
	}
}
