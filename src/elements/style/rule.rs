use core::panic;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use std::usize;

use parking_lot::RwLock;

use crate::parser::rule::Rule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::Cursor;
use crate::parser::source::Token;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

use super::custom::StyleData;
use super::custom::STYLE_CUSTOM;
use super::elem::StyleElem;
use super::state::Style;
use super::state::StyleState;
use super::state::STYLE_STATE;


#[derive(Default)]
#[auto_registry::auto_registry(registry = "rules")]
pub struct StyleRule;

impl Rule for StyleRule {
	fn name(&self) -> &'static str {
		"Style"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Inline
	}

	fn next_match(
		&self,
		unit: &TranslationUnit,
		_mode: &ParseMode,
		states: &mut CustomStates,
		cursor: &Cursor,
	) -> Option<(usize, Box<dyn std::any::Any + Send + Sync>)> {
		let source = cursor.source();
		let content = source.content();

		if !unit.has_data(STYLE_CUSTOM) {
			unit.new_data(Arc::new(RwLock::new(StyleData::default())));
		}

		let enabled = {
			if !states.contains_key(STYLE_STATE) {
				states.insert(
					STYLE_STATE.to_string(),
					Arc::new(RwLock::new(StyleState::default())),
				);
			}
			let borrow = states.get(STYLE_STATE).unwrap().read();
			borrow
				.downcast_ref::<StyleState>()
				.unwrap()
				.enabled
				.iter()
				.map(|(name, _)| name.to_owned())
				.collect::<HashSet<_>>()
		};

		unit.with_data::<StyleData, _, _>(STYLE_CUSTOM, |data| {
			let mut matched_rule = None;
			let mut closest = usize::MAX;
			data.registered.iter().for_each(|rule| {
				let re = if enabled.contains(&rule.name) {
					&rule.end_re
				} else {
					&rule.start_re
				};
				let Some(m) = re.find_at(content, cursor.pos()) else {
					return;
				};
				let start = m.start();
				if start < closest {
					matched_rule = Some(rule.clone());
					closest = start;
				}
			});

			let Some(matched) = matched_rule else {
				return None;
			};
			let active = enabled.contains(&matched.name);
			Some((closest, Box::new((matched, active)) as Box<dyn Any + Send + Sync>))
		})
	}

	fn on_match<'u>(
		&self,
		unit: &mut TranslationUnit<'u>,
		cursor: &Cursor,
		match_data: Box<dyn std::any::Any + Send + Sync>,
	) -> Cursor {
		let source = cursor.source();
		let content = source.content();

		// Get matching rule
		let (rule, active) = match_data.downcast_ref::<(Arc<Style>, bool)>().unwrap();
		let captures = if *active {
			&rule.end_re
		} else {
			&rule.start_re
		}
		.captures_at(content, cursor.pos())
		.unwrap();
		let token = Token::new(captures.get(0).unwrap().range(), cursor.source());

		// Toggle style state
		unit.get_scope()
			.with_state::<StyleState, _, _>(STYLE_STATE, |mut state| {
				if *active {
					let Some((idx, _)) = state
						.enabled
						.iter()
						.enumerate()
						.rev()
						.find(|(_, (name, _))| name == &rule.name)
					else {
						panic!()
					};
					state.enabled.remove(idx);
				} else {
					state.enabled.push((rule.name.to_owned(), token.clone()));
				}
			});

		unit.with_lsp(|lsp| lsp.with_semantics(token.source(), |sems, tokens| {
			sems.add(token.range.clone(), tokens.style_marker);
		}));
		unit.add_content(Arc::new(StyleElem {
			location: token,
			style: rule.clone(),
			enable: !*active,
		}));
		cursor.clone().at(captures.get(0).unwrap().end())
	}
}
