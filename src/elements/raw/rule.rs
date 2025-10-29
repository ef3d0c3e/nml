use ariadne::Fmt;
use regex::Captures;
use regex::Regex;

use crate::elements::raw::elem::Raw;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::RegexRule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::Token;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::unit::element::ElemKind;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

#[auto_registry::auto_registry(registry = "rules")]
pub struct RawRule {
	re: [Regex; 1],
}

impl Default for RawRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r"\{<(\w*)(?:(?:[^\S\r\n])((?:\\.|[^\\\\])*?))?>\}").unwrap()],
		}
	}
}

impl RegexRule for RawRule {
	fn name(&self) -> &'static str {
		"Raw"
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
		_index: usize,
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
		unit.with_lsp(|lsp| lsp.with_semantics(token.source(), |sems, tokens| {
			let start = captures.get(0).unwrap().start();
			sems.add(start..start+2, tokens.raw_sep);
		}));


		let Some(kind) = captures.get(1) else {
			report_err!(
				unit,
				token.source(),
				"Raw Expects Layout Kind".into(),
				span(
					captures.get(0).unwrap().range(),
					format!(
						"Expected layout kind: `{}`, `{}` or `{}` after initial `{}`",
						"inline".fg(unit.colors().info),
						"block".fg(unit.colors().info),
						"invisible".fg(unit.colors().info),
						"{<".fg(unit.colors().highlight)
					)
				)
			);
			return;
		};

		unit.with_lsp(|lsp| lsp.with_semantics(token.source(), |sems, tokens| {
			sems.add(kind.range(), tokens.raw_kind);
		}));

		let layout = match kind.as_str() {
			"inline" => ElemKind::Inline,
			"block" => ElemKind::Block,
			"invisible" => ElemKind::Invisible,
			layout => {
				report_err!(
					unit,
					token.source(),
					"Invalid Layout Type for Raw".into(),
					span(
						captures.get(0).unwrap().range(),
						format!(
							"Expected layout kind: `{}`, `{}` or `{}`, got: `{}`",
							"inline".fg(unit.colors().info),
							"block".fg(unit.colors().info),
							"invisible".fg(unit.colors().info),
							layout.fg(unit.colors().highlight)
						)
					)
				);
				return;
			}
		};

		let Some(content) = captures.get(2) else {
			report_err!(
				unit,
				token.source(),
				"Raw Expects Content".into(),
				span(
					kind.end()..captures.get(0).unwrap().end(),
					format!("Expected content after layout",)
				)
			);
			return;
		};

		unit.with_lsp(|lsp| lsp.with_semantics(token.source(), |sems, tokens| {
			let end = captures.get(0).unwrap().end();
			sems.add(content.range(), tokens.raw_content);
			sems.add(end-2..end, tokens.raw_sep);
		}));

		unit.add_content(Raw {
			location: token,
			kind: layout,
			content: content.as_str().to_string(),
		});

	}
	}
