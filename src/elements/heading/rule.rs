use std::sync::Arc;
use std::sync::OnceLock;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::RuleTarget;
use crate::parser::source::SourcePosition;
use crate::parser::source::VirtualSource;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::unit::references::InternalReference;
use crate::unit::references::Refname;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;
use ariadne::Fmt;
use parser::rule::RegexRule;
use parser::source::Token;
use parser::util::parse_paragraph;
use regex::Regex;

use super::elem::Heading;

#[auto_registry::auto_registry(registry = "rules")]
pub struct HeadingRule {
	re: [Regex; 1],
}

impl Default for HeadingRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r"(?:^|\n)(#+)(?:\{(.*)\})?(\+|\*|\+\*|\*\+)?\s+(.*)").unwrap()],
		}
	}
}

impl RegexRule for HeadingRule {
	fn name(&self) -> &'static str {
		"Heading"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Block
	}

	fn regexes(&self) -> &[Regex] {
		&self.re
	}

	fn enabled(
		&self,
		_unit: &TranslationUnit,
		mode: &ParseMode,
		_states: &mut CustomStates,
		_id: usize,
	) -> bool {
		!mode.paragraph_only
	}

	fn on_regex_match<'u>(
		&self,
		_: usize,
		unit: &mut TranslationUnit,
		token: Token,
		matches: regex::Captures,
	) {
		// Parse depth
		let depth = matches.get(1).unwrap().len();
		if depth > 6
		{
			report_err!(
				unit,
				token.source(),
				"Invalid Heading Depth".into(),
				span(
					matches.get(1).unwrap().range(),
					format!("Heading depth greater than {}", "6".fg(unit.colors().info))
				)
			);
			return;
		}
		unit.with_lsp(|lsp| lsp.with_semantics(token.source(), |sems, tokens| {
			sems.add(matches.get(1).unwrap().range(), tokens.heading_depth);
		}));

		// Parse optional refname
		let refname = if let Some(refname) = matches.get(2)
		{
			let name = match Refname::try_from(refname.as_str())
			{
				// Parse error
				Err(err) => {
					report_err!(
						unit,
						token.source(),
						"Invalid Heading Refname".into(),
						span(
							refname.range(),
							err
						)
					);
					return
				}
				// Check format
				Ok(r) => {
					let Refname::Internal(_) = r else {
						report_err!(
							unit,
							token.source(),
							"Invalid Heading Refname".into(),
							span(
								refname.range(),
								"Refname does not correspond to an internal reference name!".into()
							),
						);
						return
					};
					r
				}
			};
			unit.with_lsp(|lsp| lsp.with_semantics(token.source(), |sems, tokens| {
				sems.add(refname.range(), tokens.heading_refname);
			}));
			Some(name)
		} else { None };

		// Parse heading kind
		let (numbered, in_toc) = match matches.get(3).map_or("", |m| m.as_str())
		{
			"*+" | "+*" => (false, false),
			"*" => (false, true),
			"+" => (true, false),
			"" => (true, true),
			_ => {
				report_err!(
					unit,
					token.source(),
					"Invalid Heading Kind".into(),
					span(
						matches.get(3).unwrap().range(),
						format!("Heading kind must be a combination of {} or {}",
							"*: unnumbered".fg(unit.colors().info),
							"+: exclude form TOC".fg(unit.colors().info),
						)
					),
				);
				return
			}
		};
		if let Some(kind) = matches.get(3)
		{
			unit.with_lsp(|lsp| lsp.with_semantics(token.source(), |sems, tokens| {
				sems.add(kind.range(), tokens.heading_kind);
			}));
		}

		// Parse heading display
		let display = matches.get(4).unwrap();
		if display.as_str().trim_start().trim_end().is_empty() {
			report_warn!(
				unit,
				token.source(),
				"Empty Heading Display".into(),
				span(
					display.range(),
					"Heading display is empty".into()
				),
			);
		}

		let display_source = Arc::new(VirtualSource::new(Token::new(display.range(), token.source()), "Heading display".into(), display.as_str().into()));
		let parsed = match parse_paragraph(unit, display_source) {
			Err(err) => {
				report_err!(
					unit,
					token.source(),
					"Invalid Heading Display".into(),
					span(
						display.range(),
						format!("Failed to parse heading display:\n{err}")
					)
				);
				return;
			}
			Ok(paragraph) => paragraph,
		};

		// Reference
		let reference = if let Some(name) = refname
		{
			Some(Arc::new(InternalReference::new(
				token.source().original_range(token.range.clone()),
				name.clone(),
			)))
		} else { None };
		// Add element
		unit.add_content(Arc::new(Heading {
			location: token.clone(),
			display: vec![parsed],
			depth,
			numbered,
			in_toc,
			reference,
			link: OnceLock::default(),
		}));
	}
}
