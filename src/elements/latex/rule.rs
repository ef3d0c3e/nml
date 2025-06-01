use std::collections::HashMap;
use std::rc::Rc;
use std::str::FromStr;

use ariadne::Fmt;
use regex::Captures;
use regex::Regex;

use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::rule::RegexRule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::Token;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::parser::state::ParserState;
use crate::parser::util::escape_source;
use crate::parser::util::escape_text;
use crate::report_err;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;

use super::completion::LatexCompletion;
use super::elem::Latex;
use super::elem::TexKind;

#[auto_registry::auto_registry(registry = "rules")]
pub struct LatexRule {
	re: [Regex; 2],
	properties: PropertyParser,
}

impl Default for LatexRule {
	fn default() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"env".to_string(),
			Property::new("Tex environment".to_string(), Some("main".to_string())),
		);
		props.insert(
			"kind".to_string(),
			Property::new("Element display kind".to_string(), None),
		);
		props.insert(
			"caption".to_string(),
			Property::new("Latex caption".to_string(), None),
		);
		Self {
			re: [
				Regex::new(r"\$\|(?:\[((?:\\.|[^\\\\])*?)\])?(?:((?:\\.|[^\\\\])*?)\|\$)?")
					.unwrap(),
				Regex::new(r"\$(?:\[((?:\\.|[^\\\\])*?)\])?(?:((?:\\.|[^\\\\])*?)\$)?").unwrap(),
			],
			properties: PropertyParser { properties: props }
		}
	}
}

impl RegexRule for LatexRule {
	fn name(&self) -> &'static str {
		"Latex"
	}

	fn target(&self) -> RuleTarget {
	    RuleTarget::Inline
	}

	fn regexes(&self) -> &[regex::Regex] {
		&self.re
	}

	fn enabled(&self, _unit: &TranslationUnit, _mode: &ParseMode, _states: &mut CustomStates, _index: usize) -> bool {
		true
	}

	fn on_regex_match<'u>(
		&self,
		index: usize,
		unit: &mut TranslationUnit<'u>,
		token: Token,
		captures: Captures,
	) {
		let tex_content = match captures.get(2) {
			// Unterminated `$`
			None => {
				report_err!(
					unit,
					token.source(),
					"Unterminated Tex Code".into(),
					span(
						token.range.clone(),
						format!(
							"Missing terminating `{}` after first `{}`",
							["|$", "$"][index].fg(unit.colors().info),
							["$|", "$"][index].fg(unit.colors().info)
						)
					)
				);
				return
			}
			Some(content) => {
				let processed = escape_text(
					'\\',
					["|$", "$"][index],
					content.as_str().trim_start().trim_end(),
					true,
				);

				if processed.is_empty() {
					report_err!(
						unit,
						token.source(),
						"Empty Tex Code".into(),
						span(content.range(), "Tex code is empty".into())
					);
				}
				processed
			}
		};

		// Properties
		let prop_source = escape_source(
			token.source(),
			captures.get(1).map_or(0..0, |m| m.range()),
			"Tex Properties".into(),
			'\\',
			"]",
		);
		let Some(properties) = self.properties.parse(
			"Raw Code",
			unit,
			Token::new(0..prop_source.content().len(), prop_source),
		) else { return };

		let (Some(tex_kind), Some(tex_caption), Some(tex_env)) = (
			properties.get_or(
				unit,
				"kind",
				if index == 1 {
					TexKind::Inline
				} else {
					TexKind::Block
				},
				|_, value| TexKind::from_str(value.value.as_str()),
			),
			properties.get_opt(unit, "caption", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}),
			properties.get(unit, "env", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}),
		) else { return };

		// Code ranges
		/*
		if let Some(coderanges) = CodeRange::from_source(token.source(), &state.shared.lsp) {
			if index == 0 && tex_content.contains('\n') {
				let range = captures
					.get(2)
					.map(|m| {
						if token.source().content().as_bytes()[m.start()] == b'\n' {
							m.start() + 1..m.end()
						} else {
							m.range()
						}
					})
					.unwrap();

				coderanges.add(range, "Latex".into());
			}
		}

		state.push(
			document,
			Box::new(Tex {
				mathmode: index == 1,
				location: token.clone(),
				kind: tex_kind,
				env: tex_env,
				tex: tex_content,
				caption,
			}),
		);

		// Semantics
		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			let range = token.range;
			sems.add(
				range.start..range.start + if index == 0 { 2 } else { 1 },
				tokens.tex_sep,
			);
			if let Some(props) = captures.get(1).map(|m| m.range()) {
				sems.add(props.start - 1..props.start, tokens.tex_props_sep);
				sems.add(props.end..props.end + 1, tokens.tex_props_sep);
			}
			sems.add(captures.get(2).unwrap().range(), tokens.tex_content);
			sems.add(
				range.end - if index == 0 { 2 } else { 1 }..range.end,
				tokens.tex_sep,
			);
		}
		*/
		unit.with_lsp(|lsp| lsp.with_semantics(token.source(), |sems, tokens| {
			let range = &token.range;
			sems.add(
				range.start..range.start + if index == 0 { 2 } else { 1 },
				tokens.tex_sep,
			);
			if let Some(props) = captures.get(1).map(|m| m.range()) {
				sems.add(props.start - 1..props.start, tokens.tex_prop_sep);
				sems.add(props.end..props.end + 1, tokens.tex_prop_sep);
			}
			sems.add(captures.get(2).unwrap().range(), tokens.tex_content);
			sems.add(
				range.end - if index == 0 { 2 } else { 1 }..range.end,
				tokens.tex_sep,
			);
		}));

		unit.add_content(Rc::new(Latex {
			location: token,
			mathmode: index == 1,
			kind: tex_kind,
			env: tex_env,
			tex: tex_content,
			caption: tex_caption,
		}));
	}

	fn completion(&self) -> Option<Box<dyn lsp::completion::CompletionProvider + 'static + Send + Sync>> {
	    Some(Box::new(LatexCompletion{}))
	}
}
