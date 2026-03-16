use std::collections::HashMap;
use std::str::FromStr;

use ariadne::Fmt;
use regex::Captures;
use regex::Regex;

use crate::elements::typst::elem::Typst;
use crate::elements::typst::elem::TypstKind;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::rule::RegexRule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::Token;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::parser::util::escape_source;
use crate::parser::util::escape_text;
use crate::report_err;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;

#[auto_registry::auto_registry(registry = "rules")]
pub struct TypstRule {
	re: [Regex; 1],
	properties: PropertyParser,
}

impl Default for TypstRule {
	fn default() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"env".to_string(),
			Property::new("Typst environment".to_string(), Some("main".to_string())),
		);
		props.insert(
			"kind".to_string(),
			Property::new("Element display kind".to_string(), None),
		);
		props.insert(
			"caption".to_string(),
			Property::new("Typst caption".to_string(), None),
		);
		Self {
			re: [
				Regex::new(r"\$\[(?:\[((?:\\.|[^\\\\])*?)\])?(?:((?:\\.|[^\\\\])*?)(\]\$))?")
					.unwrap(),
			],
			properties: PropertyParser { properties: props },
		}
	}
}

impl RegexRule for TypstRule {
	fn name(&self) -> &'static str {
		"Typst"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Inline
	}

	fn before(&self) -> Option<&'static str> {
	    Some("Latex")
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
		index: usize,
		unit: &mut TranslationUnit,
		token: Token,
		captures: Captures,
	) {
		let typ_content = match captures.get(2) {
			// Unterminated `$[`
			None => {
				report_err!(
					unit,
					token.source(),
					"Unterminated Typst Code".into(),
					span(
						token.range.clone(),
						format!(
							"Missing terminating `{}` after first `{}`",
							"$]".fg(unit.colors().info),
							"$[".fg(unit.colors().info),
						)
					)
				);
				return;
			}
			Some(content) => {
				let processed = escape_text(
					'\\',
					"]$",
					content.as_str().trim_start().trim_end(),
					true,
				);

				if processed.is_empty() {
					report_err!(
						unit,
						token.source(),
						"Empty Typst Code".into(),
						span(content.range(), "Typst code is empty".into())
					);
				}
				processed
			}
		};

		// Properties
		let prop_source = escape_source(
			token.source(),
			captures.get(1).map_or(0..0, |m| m.range()),
			"Typst Properties".into(),
			'\\',
			"]",
		);
		let Some(mut properties) = self.properties.parse(
			"Raw Code",
			unit,
			Token::new(0..prop_source.content().len(), prop_source),
		) else {
			return;
		};

		let (Some(typ_kind), Some(typ_caption), Some(typ_env)) = (
			properties.get_or(
				unit,
				"kind",
				if index == 1 {
					TypstKind::Inline
				} else {
					TypstKind::Block
				},
				|_, value| TypstKind::from_str(value.value.as_str()),
			),
			properties.get_opt(unit, "caption", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}),
			properties.get(unit, "env", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}),
		) else {
			return;
		};

		unit.with_lsp(|lsp| {
			lsp.with_semantics(token.source(), |sems, tokens| {
				let range = &token.range;
				sems.add(
					range.start..range.start + if index == 0 { 2 } else { 1 },
					tokens.typ_sep,
				);
				if let Some(props) = captures.get(1).map(|m| m.range()) {
					sems.add(props.start - 1..props.start, tokens.typ_prop_sep);
					sems.add(props.end..props.end + 1, tokens.typ_prop_sep);
				}
				sems.add(captures.get(2).unwrap().range(), tokens.typ_content);
				sems.add(
					range.end - if index == 0 { 2 } else { 1 }..range.end,
					tokens.typ_sep,
				);
			})
		});

		unit.add_content(Typst {
			location: token,
			mathmode: index == 1,
			kind: typ_kind,
			env: typ_env,
			typ: typ_content,
			caption: typ_caption,
		});
	}

	//fn completion(
	//	&self,
	//) -> Option<Box<dyn lsp::completion::CompletionProvider + 'static + Send + Sync>> {
	//	Some(Box::new(LatexCompletion {}))
	//}
}
