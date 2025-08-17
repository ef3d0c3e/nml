use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use ariadne::Fmt;
use regex::Regex;

use crate::elements::code::elem::Code;
use crate::elements::code::elem::CodeDisplay;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::RegexRule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::Token;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::parser::util::escape_text;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

#[auto_registry::auto_registry(registry = "rules")]
pub struct CodeRule {
	re: [Regex; 3],
	properties: PropertyParser,
}

impl Default for CodeRule {
	fn default() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"line_offset".to_string(),
			Property::new("Code line offset".to_string(), Some("0".to_string())),
		);
		Self {
			re: [
				Regex::new(r"``(?:\[((?:\\.|[^\\])*?)\])?([^,\r\n]*),((?:\\.|[^\\\n\r])*?)``")
					.unwrap(),
				Regex::new(
					r"(?:\n|^)```(?:\[((?:\\.|[^\\\\])*?)\])?([^,\r\n]*)(?:,(.*))?(?:\n((?:\\.|[^\\\\])*?)```)?",
				)
				.unwrap(),
				Regex::new(
					r"(?:\n|^)``(?:\[((?:\\.|[^\\\\])*?)\])?([^,\r\n]*)(?:,(.*))?(?:\n((?:\\.|[^\\\\])*?)``)?",
				)
				.unwrap(),
			],
			properties: PropertyParser { properties: props },
		}
	}
}

impl RegexRule for CodeRule {
	fn name(&self) -> &'static str {
		"Code"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Command
	}

	fn on_regex_match<'u>(
		&self,
		index: usize,
		unit: &mut TranslationUnit,
		token: Token,
		captures: regex::Captures,
	) {
		// Parse properties
		let Some(mut props) = self.properties.parse_token(
			"Code",
			unit,
			Token::new(captures.get(1).map_or(0..0, |m| m.range()), token.source()),
			'\\',
			"]",
		) else {
			return;
		};
		let Some(line_offset) =
			props.get(unit, "line_offset", |_, value| value.value.parse::<usize>())
		else {
			return;
		};

		// Parse language
		let language = match captures.get(2) {
			None => "Plain Text",
			Some(language) => {
				let mut lang = language.as_str().trim_start().trim_end();
				if lang.is_empty() {
					lang = "Plain Text"
				};
				if Code::syntaxes().find_syntax_by_name(lang).is_none() {
					report_err!(
						unit,
						token.source(),
						"Unknown Code Language".into(),
						span(
							language.range(),
							format!("Language `{}` cannot be found", lang)
						)
					);
					return;
				}
				lang
			}
		}
		.to_string();

		// Parse title for block mode
		let title = (index > 0).then_some(
			captures
				.get(3)
				.map_or("", |m| m.as_str())
				.trim_start()
				.trim_end()
				.to_string(),
		);

		// Parse content
		let closing = if index == 1 { "```" } else { "``" };
		let Some(content) = captures.get(4).map(|m| m.as_str()) else {
			report_err!(
				unit,
				token.source(),
				"Missing Code Content".into(),
				span(
					captures.get(0).unwrap().range(),
					format!(
						"Expected code content after opening '{}'",
						closing.fg(unit.colors().highlight)
					)
				)
			);
			return;
		};
		let content = escape_text('\\', closing, content, false);

		unit.add_content(Arc::new(Code {
			location: token,
			language,
			display: CodeDisplay {
				title,
				line_gutter: index == 1,
				line_offset,
				inline: index == 0,
			},
			content,
		}));
	}

	fn regexes(&self) -> &[regex::Regex] {
		&self.re
	}

	fn enabled(
		&self,
		_unit: &TranslationUnit,
		mode: &ParseMode,
		_states: &mut CustomStates,
		index: usize,
	) -> bool {
		index == 0 || !mode.paragraph_only
	}
}
