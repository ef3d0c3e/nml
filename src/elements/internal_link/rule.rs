use crate::elements::text::elem::Text;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::RuleTarget;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::parser::util::escape_source;
use crate::parser::util::parse_paragraph;
use crate::unit::references::Refname;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;
use ariadne::Fmt;
use ariadne::Span;
use regex::Captures;
use regex::Regex;
use std::sync::Arc;
use std::sync::OnceLock;

use crate::parser::reports::Report;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;

use super::completion::ReferenceCompletion;
use super::elem::InternalLink;

#[auto_registry::auto_registry(registry = "rules")]
pub struct InternalLinkRule {
	re: [Regex; 1],
}

impl Default for InternalLinkRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r"&\{([^\}\r\n]*)(\})?(?:(\[)((?:[^]\\]|\\.)*)(\])?)?").unwrap()],
		}
	}
}

impl RegexRule for InternalLinkRule {
	fn name(&self) -> &'static str {
		"Inernal Link"
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
		let link = captures.get(1).unwrap();

		// Missing '}'
		if captures.get(2).is_none() {
			report_err!(
				unit,
				token.source(),
				"Invalid internal link".into(),
				span(
					link.end()..link.end() + 1,
					format!("Missing closing `{}`", "}".fg(unit.colors().info))
				),
				span_highlight(
					link.start() - 1..link.start(),
					format!("Opening `{}` here", "{".fg(unit.colors().highlight))
				),
				note("While attempting to parse internal link path".into())
			);
			return;
		}

		let link_content = link.as_str().trim_start().trim_end();
		// Empty link
		if link_content.is_empty() {
			report_err!(
				unit,
				token.source(),
				"Invalid internal link".into(),
				span(link.range(), format!("Expected path, found empty string"))
			);
			return;
		}
		// Link to refname
		let link_refname = match Refname::try_from(link_content) {
			Ok(refname) => {
				if let Refname::Internal(name) = &refname {
					if let Some(reference) = unit.get_reference(name) {
						unit.with_lsp(|mut lsp| {
							ReferenceCompletion::export_internal_ref(unit, &mut lsp, reference);
						});
					}
				}

				refname
			}
			Err(err) => {
				report_err!(
					unit,
					token.source(),
					"Invalid internal link".into(),
					span(link.range(), err),
					note("While attempting to parse internal link path".into())
				);
				return;
			}
		};

		unit.with_lsp(|lsp| {
			lsp.with_semantics(token.source(), |sems, tokens| {
				let range = captures.get(0).unwrap().range();
				sems.add(
					range.start()..range.start() + 2,
					tokens.internal_link_ref_sep,
				);
				sems.add(link.range(), tokens.internal_link_ref);
				sems.add(link.end()..link.end() + 1, tokens.internal_link_ref_sep);
			})
		});

		// Custom display, if '[' present
		let display = if captures.get(3).is_some() {
			let display = captures.get(4).unwrap();

			// Missing ']'
			if captures.get(5).is_none() {
				report_err!(
					unit,
					token.source(),
					"Invalid internal link".into(),
					span(
						display.end() - 1..display.end(),
						format!("Missing closing `{}`", "]".fg(unit.colors().info))
					),
					span_highlight(
						display.start() - 1..display.start(),
						format!("Opening `{}` here", "[".fg(unit.colors().highlight))
					),
					note("While attempting to parse internal link display".into())
				);
				return;
			}
			unit.with_lsp(|lsp| {
				lsp.with_semantics(token.source(), |sems, tokens| {
					sems.add(
						display.start() - 1..display.start(),
						tokens.internal_link_display_sep,
					);
					sems.add_to_queue(
						display.end()..display.end() + 1,
						tokens.internal_link_display_sep,
					);
				})
			});
			let display_source = escape_source(
				token.source(),
				display.range(),
				"Internal Link Display".into(),
				'\\',
				"]",
			);
			match parse_paragraph(unit, display_source) {
				Err(err) => {
					report_err!(
						unit,
						token.source(),
						"Invalid internal link display".into(),
						span(
							display.range(),
							format!("Failed to parse internal link display:\n{err}")
						)
					);
					return;
				}
				Ok(paragraph) => paragraph,
			}
		}
		// Default display
		else {
			let display_source = token.to_source(format!(
				"Internal link display for `{}`",
				link_refname.to_string()
			));
			// Add content to scope
			unit.with_child(
				display_source.clone(),
				ParseMode::default(),
				true,
				|_unit, scope| {
					scope.add_content(Arc::new(Text {
						location: display_source.into(),
						content: link_refname.to_string(),
					}));
					scope
				},
			)
		};

		unit.with_lsp(|lsp| {
			let label = link_refname.to_string();
			let Some(reference) = lsp.external_refs.get(&label) else {
				return;
			};
			let Some(ref_source) = lsp.get_source(&reference.source_path) else {
				return;
			};

			let source = Token::new(link.range(), token.source());
			lsp.add_definition(
				source.clone(),
				&Token::new(reference.range.clone(), ref_source),
			);
			lsp.add_hover(
				source,
				ReferenceCompletion::get_documentation(reference, &label),
			);
		});

		unit.add_content(InternalLink {
			location: token.clone(),
			refname: link_refname,
			display: vec![display],
			reference: OnceLock::new(),
		});
	}

	fn completion(
		&self,
	) -> Option<Box<dyn lsp::completion::CompletionProvider + 'static + Send + Sync>> {
		Some(Box::new(ReferenceCompletion {}))
	}
}
