use std::any::Any;
use std::ops::Range;
use std::slice::Iter;
use std::sync::Arc;
use std::usize;

use graphviz_rust::print;

use crate::elements::meta::eof::Eof;
use crate::elements::text::elem::Text;
use crate::lsp::completion::CompletionProvider;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

use super::rule::Rule;
use super::source::Cursor;
use super::source::Token;

pub struct Parser {
	rules: Vec<Box<dyn Rule + Send + Sync>>,
}

impl Parser {
	/// Constructs a new parser that will automatically fetch all exported rules
	pub fn new() -> Self {
		let rules = super::rule::get_rule_registry();
		// Register lua bindings
		for rule in &rules {
			rule.register_bindings();
		}
		Self { rules }
	}

	/// Updates matches from a given start position e.g [`Cursor`]
	///
	/// # Return
	///
	///  1. The cursor position after updating the matches
	///  2. (Optional) The winning match with it's match data
	/// If the winning match is None, it means that the document has no more
	/// rule to match. I.e The rest of the content should be added as a
	/// [`crate::elements::text::elem::Text`] element.
	/// The match data should be passed to the [`Rule::on_match`] method.
	///
	/// # Strategy
	///
	/// This function call [`Rule::next_match`] on the rules defined for the
	/// parser. It then takes the rule that has the closest `next_match` and
	/// returns it. If `next_match` starts on an escaped character i.e `\\`,
	/// then it starts over to find another match for that rule.
	/// In case multiple rules have the same `next_match`, the rules that are
	/// defined first in the parser are prioritized. See [`Rule::previous`] for
	/// information on how to prioritize rules.
	/// When multiple rules have the same 'next_match', it prioritizes the one that matched the longest content
	///
	/// Notes that the result of every call to [`Rule::next_match`] gets stored
	/// in a table: [`ParserState::matches`]. Until the cursor steps over a
	/// position in the table, `next_match` won't be called.
	fn next_match(
		&self,
		unit: &mut TranslationUnit,
		cursor: &Cursor,
	) -> Option<(
		Cursor,
		&Box<dyn Rule + Send + Sync>,
		Box<dyn Any + Send + Sync>,
	)> {
		let mut scope = unit.get_scope().write();
		let state = scope.parser_state_mut();
		// Initialize state if required
		while state.matches.len() < self.rules.len() {
			state.matches.push((0..0, None));
		}

		self.rules
			.iter()
			.zip(state.matches.iter_mut())
			.for_each(|(rule, (range, data))| {
				// Don't upate if not stepped over yet
				if range.start > cursor.pos() {
					return;
				}
				// Update next match position
				(*range, *data) =
					match rule.next_match(unit, &state.mode, &mut state.states, cursor) {
						None => (usize::MAX..usize::MAX, None),
						Some((mut new_range, mut new_data)) => {
							let mut local_cursor = cursor.to_owned();
							// Check if escaped
							while local_cursor.pos() != usize::MAX {
								let source = cursor.source();
								let content = source.content().as_str();

								let mut codepoints = content[0..new_range.start].chars();
								let mut escaped = false;

								'inner: loop {
									let g = codepoints.next_back();
									if g.is_none() || g.unwrap() != '\\' {
										break 'inner;
									}
									escaped = !escaped;
								}
								if !escaped {
									break;
								}
								// Advance by 1 codepoint if escaped
								match content[new_range.start..].chars().next() {
									Some(ch) => {
										local_cursor =
											local_cursor.at(local_cursor.pos() + ch.len_utf8())
									}
									None => panic!(),
								};
								// Find next potential match
								(new_range, new_data) = match rule.next_match(
									unit,
									&state.mode,
									&mut state.states,
									&local_cursor,
								) {
									None => (usize::MAX..usize::MAX, new_data), // Stop iterating
									Some((new_range, new_data)) => (new_range, new_data),
								};
								local_cursor = local_cursor.at(new_range.start);
							}
							(new_range, Some(new_data))
						}
					};
			});

		// Get winning match
		let mut next = usize::MAX;
		let mut length = 0;
		let mut best_idx = 0;
		for (idx, state) in state.matches.iter().enumerate() {
			// Update if better
			if state.0.start < next {
				next = state.0.start;
				length = state.0.end - state.0.start;
				best_idx = idx;
			}
			// On conflict pick the longest match
			else if state.0.start == next {
				let cur_length = state.0.end - state.0.start;
				if cur_length > length {
					next = state.0.start;
					length = cur_length;
					best_idx = idx;
				}
			}
		}
		if next == usize::MAX {
			return None;
		}

		return Some((
			cursor.at(next),
			&self.rules[best_idx],
			state.matches[best_idx].1.take().unwrap(),
		));
	}

	/// Adds content from `range` as text to `unit`
	fn add_text<'u>(&'u self, unit: &mut TranslationUnit, range: Range<Cursor>) {
		let token: Token = (&range).into();
		let mut first = true;
		let mut content = token.content().chars().fold(String::default(), {
			let mut escaped = false;
			move |mut s, c| {
				if c == '\\' && !escaped {
					escaped = !escaped;
				} else if escaped {
					s.push(c);
					escaped = false;
					first = false;
				} else if c == '\n' {
					if !first {
						s.push(' ');
					}
				} else {
					s.push(c);
					first = false;
				}
				s
			}
		});
		content = content.as_str().to_string();

		if content.is_empty() {
			return;
		}

		unit.add_content(Text::new(token, content.into()));
	}

	/// Parses the current scope in the translation unit
	pub fn parse(&self, unit: &mut TranslationUnit) {
		let mut cursor = Cursor::new(0, Arc::as_ref(unit.get_scope()).read().source().into());

		while let Some((next_cursor, rule, rule_data)) = self.next_match(unit, &cursor) {
			// Unmatched content added as text
			self.add_text(unit, cursor..next_cursor.clone());

			// Trigger rule
			cursor = rule.on_match(unit, &next_cursor, rule_data);
		}
		// Add leftover as text
		let end_cursor = cursor.at(cursor.source().content().len());
		self.add_text(unit, cursor..end_cursor.clone());

		unit.get_scope().add_content(Arc::new(Eof {
			location: Token::new(end_cursor.pos()..end_cursor.pos(), end_cursor.source()),
		}));

		// Trigger the end of document for the semantics
		//unit.with_lsp(|lsp| {
		//	lsp.
		//});
	}

	/// Get completion providers for this parser
	pub fn get_completors(&self) -> Vec<Box<dyn CompletionProvider + 'static + Send + Sync>> {
		let mut completors = vec![];

		self.rules.iter().for_each(|rule| {
			let Some(completor) = rule.completion() else {
				return;
			};
			completors.push(completor);
		});
		completors
	}
}

pub trait ParserRuleAccessor {
	fn rules_iter(&self) -> Iter<'_, Box<dyn Rule + Send + Sync>>;
}

impl ParserRuleAccessor for Parser {
	fn rules_iter(&self) -> Iter<'_, Box<dyn Rule + Send + Sync>> {
		self.rules.iter()
	}
}
