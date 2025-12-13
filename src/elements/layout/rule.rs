use std::sync::Arc;

use ariadne::Fmt;
use graphviz_rust::cmd::Layout;
use parking_lot::RwLock;
use regex::Regex;

use crate::elements::layout::custom::LayoutData;
use crate::elements::layout::custom::LAYOUT_CUSTOM;
use crate::elements::layout::elem::LayoutElem;
use crate::elements::layout::elem::LayoutToken;
use crate::elements::layout::state::LayoutState;
use crate::elements::layout::state::LAYOUT_STATE;
use crate::parser::rule::RegexRule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::Token;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::report_err;

#[auto_registry::auto_registry(registry = "rules")]
pub struct LayoutRule {
	re: [Regex; 1],
}

impl Default for LayoutRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r"(?:^|\n)(:layout)(?:[^\S\r\n]+(\w+)(?:[^\S\r\n]+(.*))?)?").unwrap()],
		}
	}
}

impl RegexRule for LayoutRule {
	fn name(&self) -> &'static str {
		"Layout"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Command
	}

	fn regexes(&self) -> &[regex::Regex] {
		&self.re
	}

	fn enabled(
		&self,
		_unit: &TranslationUnit,
		mode: &ParseMode,
		states: &mut CustomStates,
		_index: usize,
	) -> bool {
		if !states.contains_key(LAYOUT_STATE) {
			states.insert(
				LAYOUT_STATE.to_string(),
				Arc::new(RwLock::new(LayoutState::default())),
			);
		}

		!mode.paragraph_only
	}

	fn on_regex_match<'u>(
		&self,
		_index: usize,
		unit: &mut TranslationUnit,
		token: Token,
		captures: regex::Captures,
	) {
		if !unit.has_data(LAYOUT_CUSTOM) {
			unit.new_data(Arc::new(RwLock::new(LayoutData::default())));
		}

		let Some(command) = captures.get(2) else {
			report_err!(
				unit,
				token.source(),
				"Invalid Layout".into(),
				span(token.range.clone(), format!("Expected layout command",))
			);
			return;
		};
		let opts = captures.get(3);

		unit.with_lsp(|lsp| lsp.with_semantics(token.source(),|sems, tokens| {
			let cmd = captures.get(1).unwrap();
			sems.add(cmd.range(), tokens.command);
			sems.add(command.range(), tokens.layout_type);
			if let Some(opts) = opts
			{
				sems.add(opts.range(), tokens.layout_opts);
			}
		}));

		let data = unit.get_data(LAYOUT_CUSTOM);
		let mut lock = data.write();
		let data = lock.downcast_mut::<LayoutData>().unwrap();

		// :layout end
		if command.as_str() == "end" {
			if let Some(opts) = opts {
				report_err!(
					unit,
					token.source(),
					"Invalid Layout".into(),
					span(
						opts.range(),
						format!(
							"Unexpected options for `{}`",
							":layout end".fg(unit.colors().highlight)
						)
					)
				);
				return;
			}

			let Some(layout) = unit
				.get_scope()
				.with_state::<LayoutState, _, _>(LAYOUT_STATE, |mut state| state.state.pop())
			else {
				report_err!(
					unit,
					token.source(),
					"Invalid Layout".into(),
					span(
						token.range.clone(),
						format!(
							"{} without an active layout",
							":layout end".fg(unit.colors().highlight)
						)
					)
				);
				return;
			};
			unit.add_content(LayoutElem {
				location: token.clone(),
				id: layout.2,
				layout: layout.0,
				token: LayoutToken::End,
				params: None,
			});
			return;
		}

		// :layout next
		if command.as_str() == "next" {
			let Some(layout) = unit
				.get_scope()
				.with_state::<LayoutState, _, _>(LAYOUT_STATE, |mut state| {
					state.state.last().cloned()
				})
			else {
				report_err!(
					unit,
					token.source(),
					"Invalid Layout".into(),
					span(
						token.range.clone(),
						format!(
							"`{}` with no active layout",
							":layout next".fg(unit.colors().highlight)
						)
					)
				);
				return;
			};
			if layout.2 + 1 >= layout.0.expects().end {
				report_err!(
					unit,
					token.source(),
					"Invalid Layout".into(),
					span(
						token.range.clone(),
						format!(
							"Layout `{}` expects between {} and {} blocks",
							layout.0.name().fg(unit.colors().highlight),
							layout.0.expects().start.fg(unit.colors().info),
							layout.0.expects().end.fg(unit.colors().info),
						)
					)
				);
				return;
			}
			let opts_token = match opts {
				Some(opts) => Token::new(opts.range(), token.source()),
				None => Token::new(token.range.end..token.range.end, token.source()),
			};
			let Some(result) = layout.0.parse_properties(unit, opts_token, layout.2) else {
				return;
			};
			unit.get_scope()
				.with_state::<LayoutState, _, _>(LAYOUT_STATE, |mut state| {
					state.state.pop();
					state
						.state
						.push((layout.0.clone(), layout.1.clone(), layout.2 + 1));
				});
			unit.add_content(LayoutElem {
				location: token.clone(),
				id: layout.2,
				layout: layout.0,
				token: LayoutToken::Next,
				params: Some(result),
			});
			return;
		}

		let Some(layout) = data.registered.get(command.as_str()) else {
			report_err!(
				unit,
				token.source(),
				"Invalid Layout".into(),
				span(
					token.range.clone(),
					format!(
						"Unknown layout type `{}`",
						command.as_str().fg(unit.colors().highlight)
					)
				)
			);
			return;
		};

		let opts_token = match opts {
			Some(opts) => Token::new(opts.range(), token.source()),
			None => Token::new(token.range.end..token.range.end, token.source()),
		};
		let Some(result) = layout.parse_properties(unit, opts_token, 0) else {
			return;
		};
		unit.get_scope()
			.with_state::<LayoutState, _, _>(LAYOUT_STATE, |mut state| {
				state.state.push((layout.clone(), token.clone(), 0));
			});
		unit.add_content(LayoutElem {
			location: token.clone(),
			id: 0,
			layout: layout.clone(),
			token: LayoutToken::Start,
			params: Some(result),
		});
	}
}
