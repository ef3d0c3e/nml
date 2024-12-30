use crate::document::references::CrossReference;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use lsp::semantic::Semantics;
use parser::property::PropertyParser;
use parser::util::escape_source;
use regex::Captures;
use regex::Regex;
use std::collections::HashMap;

use crate::document::document::Document;
use crate::document::references::validate_refname;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::property::Property;
use crate::parser::reports::Report;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;

use super::elem::ExternalReference;
use super::elem::InternalReference;
use super::style::ExternalReferenceStyle;

#[auto_registry::auto_registry(registry = "rules")]
pub struct ReferenceRule {
	re: [Regex; 1],
	properties: PropertyParser,
}

impl Default for ReferenceRule {
	fn default() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"caption".to_string(),
			Property::new("Override the display of the reference".to_string(), None),
		);
		Self {
			re: [Regex::new(r"&\{(.*?)\}(?:\[((?:\\.|[^\\\\])*?)\])?").unwrap()],
			properties: PropertyParser { properties: props },
		}
	}
}

impl RegexRule for ReferenceRule {
	fn name(&self) -> &'static str { "Reference" }

	fn previous(&self) -> Option<&'static str> { Some("Text") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn enabled(&self, _mode: &ParseMode, _id: usize) -> bool { true }

	fn on_regex_match<'a>(
		&self,
		_: usize,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		let (refdoc, refname) = if let Some(refname_match) = matches.get(1) {
			if let Some(sep) = refname_match.as_str().find('#')
			// External reference
			{
				let refdoc = refname_match.as_str().split_at(sep).0;
				match validate_refname(document, refname_match.as_str().split_at(sep + 1).1, false)
				{
					Err(err) => {
						report_err!(
							&mut reports,
							token.source(),
							"Invalid Reference Refname".into(),
							span(refname_match.range(), err)
						);
						return reports;
					}
					Ok(refname) => (Some(refdoc.to_string()), refname.to_string()),
				}
			} else
			// Internal reference
			{
				match validate_refname(document, refname_match.as_str(), false) {
					Err(err) => {
						report_err!(
							&mut reports,
							token.source(),
							"Invalid Reference Refname".into(),
							span(refname_match.range(), err)
						);
						return reports;
					}
					Ok(refname) => (None, refname.to_string()),
				}
			}
		} else {
			panic!("Unknown error")
		};

		// Properties
		let prop_source = escape_source(
			token.source(),
			matches.get(2).map_or(0..0, |m| m.range()),
			"Reference Properties".into(),
			'\\',
			"]",
		);
		let properties = match self.properties.parse(
			"Reference",
			&mut reports,
			state,
			Token::new(0..prop_source.content().len(), prop_source),
		) {
			Some(props) => props,
			None => return reports,
		};

		let caption = match properties.get_opt(&mut reports, "caption", |_, value| {
			Result::<_, String>::Ok(value.value.clone())
		}) {
			Some(caption) => caption,
			None => return reports,
		};

		if let Some(refdoc) = refdoc {
			// Get style
			let style = state
				.shared
				.styles
				.borrow()
				.current(ExternalReferenceStyle::key())
				.downcast_rc::<ExternalReferenceStyle>()
				.unwrap();

			// &{#refname}
			if refdoc.is_empty() {
				state.push(
					document,
					Box::new(ExternalReference {
						location: token.clone(),
						reference: CrossReference::Unspecific(refname),
						caption,
						style,
					}),
				);
			// &{docname#refname}
			} else {
				state.push(
					document,
					Box::new(ExternalReference {
						location: token.clone(),
						reference: CrossReference::Specific(refdoc.clone(), refname),
						caption,
						style,
					}),
				);
			}

			if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp)
			{
				let link = matches.get(1).unwrap().range();
				sems.add(link.start - 2..link.start - 1, tokens.reference_operator);
				sems.add(link.start - 1..link.start, tokens.reference_link_sep);

				if !refdoc.is_empty() {
					sems.add(link.start..refdoc.len() + link.start, tokens.reference_doc);
				}
				sems.add(
					refdoc.len() + link.start..refdoc.len() + link.start + 1,
					tokens.reference_doc_sep,
				);
				sems.add(
					refdoc.len() + link.start + 1..link.end,
					tokens.reference_link,
				);
				sems.add(link.end..link.end + 1, tokens.reference_link_sep);
			}
		} else {
			state.push(
				document,
				Box::new(InternalReference {
					location: token.clone(),
					refname,
					caption,
				}),
			);

			if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp)
			{
				let link = matches.get(1).unwrap().range();
				sems.add(link.start - 2..link.start - 1, tokens.reference_operator);
				sems.add(link.start - 1..link.start, tokens.reference_link_sep);
				sems.add(link.clone(), tokens.reference_link);
				sems.add(link.end..link.end + 1, tokens.reference_link_sep);
			}
		}

		if let (Some((sems, tokens)), Some(props)) = (
			Semantics::from_source(token.source(), &state.shared.lsp),
			matches.get(2).map(|m| m.range()),
		) {
			sems.add(props.start - 1..props.start, tokens.reference_props_sep);
			sems.add(props.end..props.end + 1, tokens.reference_props_sep);
		}

		reports
	}
}
