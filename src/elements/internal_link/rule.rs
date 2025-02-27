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

use crate::parser::reports::Report;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;

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
	fn name(&self) -> &'static str { "Inernal Link" }

	fn previous(&self) -> Option<&'static str> { Some("Link") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn enabled(&self, _mode: &ParseMode, _id: usize) -> bool { true }

	fn on_regex_match<'u>(
		&self,
		_index: usize,
		unit: &mut TranslationUnit<'u>,
		token: Token,
		captures: Captures,
	) {
		let link = captures.get(1).unwrap();

		// Missing '}'
		if captures.get(2).is_none()
		{
			report_err!(
				unit,
				token.source(),
				"Invalid internal link".into(),
				span(
					link.end()..link.end()+1,
					format!("Missing closing `{}`", "}".fg(unit.colors().info))
				),
				span_highlight(
					link.start()-1..link.start(),
					format!("Opening `{}` here", "{".fg(unit.colors().highlight))
				),
				note("While attempting to parse internal link path".into())
			);
			return
		}

		let link_content = link.as_str().trim_start().trim_end();
		// Empty link
		if link_content.is_empty()
		{
			report_err!(
				unit,
				token.source(),
				"Invalid internal link".into(),
				span(
					link.range(),
					format!("Expected path, found empty string")
				)
			);
			return
		}
		// Link to refname
		let link_refname = match Refname::try_from(link_content)
		{
			Ok(refname) => refname,
			Err(err) => {
				report_err!(
					unit,
					token.source(),
					"Invalid internal link".into(),
					span(
						link.range(),
						err
					),
					note("While attempting to parse internal link path".into())
				);
				return
			}
		};

		let resolved = if let Refname::Internal(_) = &link_refname
		{
			unit.get_reference(&link_refname)
				.map(|reference| reference.reference())
		} else { None };

		// Custom display, if '[' present
		let display = if captures.get(3).is_some()
		{
			let display = captures.get(4).unwrap();

			// Missing ']'
			if captures.get(5).is_none()
			{
				report_err!(
					unit,
					token.source(),
					"Invalid internal link".into(),
					span(
						display.end()-1..display.end(),
						format!("Missing closing `{}`", "]".fg(unit.colors().info))
					),
					span_highlight(
						display.start()-1..display.start(),
						format!("Opening `{}` here", "[".fg(unit.colors().highlight))
					),
					note("While attempting to parse internal link display".into())
				);
				return
			}
			let display_source = escape_source(
				token.source(),
				display.range(),
				"Internal Link Display".into(),
				'\\',
				"]");
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
		else
		{
			let display_source = token.to_source(format!("Internal link display for `{}`", link_refname.to_string()));
			// Add content to scope
			unit.with_child(display_source.clone(), ParseMode::default(), true, |unit, scope| {
				scope.add_content(Rc::new(Text{
					location: display_source.into(),
					content: link_refname.to_string(),
				}));
				scope
			})
		};

		unit.add_content(Rc::new(InternalLink {
			location: token.clone(),
			refname: link_refname,
			display: vec![display],
			resolved,
		}));
	}
}
