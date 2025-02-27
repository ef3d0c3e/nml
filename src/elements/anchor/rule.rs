use crate::document::references::InternalReference;
use crate::document::references::Refname;
use crate::elements::text::elem::Text;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::scope::ScopeAccessor;
use crate::parser::state::ParseMode;
use crate::parser::translation::TranslationAccessors;
use crate::parser::translation::TranslationUnit;
use crate::parser::util::escape_source;
use crate::parser::util::parse_paragraph;
use ariadne::Fmt;
use regex::Captures;
use regex::Regex;
use std::rc::Rc;
use std::rc::Weak;

use crate::parser::reports::Report;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;

use super::elem::Anchor;

#[auto_registry::auto_registry(registry = "rules")]
pub struct AnchorRule {
	re: [Regex; 1],
}

impl Default for AnchorRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r":anchor[^\S\r\n]+([^:\r\n]*)?(:)?").unwrap()],
		}
	}
}

impl RegexRule for AnchorRule {
	fn name(&self) -> &'static str { "Anchor" }

	fn previous(&self) -> Option<&'static str> { Some("Break") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn enabled(&self, _mode: &ParseMode, _id: usize) -> bool { true }

	fn on_regex_match<'u>(
		&self,
		_index: usize,
		unit: &mut TranslationUnit<'u>,
		token: Token,
		captures: Captures,
	) {
		let anchor = captures.get(1).unwrap();

		// Missing ':'
		if captures.get(2).is_none()
		{
			report_err!(
				unit,
				token.source(),
				"Invalid anchor".into(),
				span(
					token.end()..anchor.end()+1,
					format!("Missing closing `{}`", ":".fg(unit.colors().info))
				),
				span_highlight(
					token.start()..token.start()+1,
					format!("Opening `{}` here", ":".fg(unit.colors().highlight))
				),
				note("While attempting to parse anchor".into())
			);
			return
		}

		// Parse to refname
		let anchor_refname = match Refname::try_from(anchor.as_str())
		{
			// Parse error
			Err(err) => {
				report_err!(
					unit,
					token.source(),
					"Invalid anchor".into(),
					span(
						anchor.range(),
						err
					),
					note("While attempting to parse anchor".into())
				);
				return
			},
			// Check format
			Ok(r) => match r {
				Refname::Internal(_) => r,
				_ =>  {
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
					return
				},
			},
		};

		let reference = Rc::new(InternalReference {
			location: token.clone(),
			refname: anchor_refname.clone(),
		});
		let mut elem = Rc::new(Anchor {
			location: token.clone(),
			refname: anchor_refname.clone(),
			reference: reference.clone(),
		});
		unit.add_content(elem);
		unit.get_scope()
			.insert_reference(reference);
	}
}
