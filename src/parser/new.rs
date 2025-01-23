use std::any::Any;
use std::ops::Range;
use std::rc::Rc;
use std::slice::Iter;
use std::sync::Arc;

use crate::elements::text::elem::Text;

use super::reports::Report;
use super::rule::Rule;
use super::scope::ScopeAccessor;
use super::source::Cursor;
use super::source::Token;
use super::state::ParserState;
use super::translation::TranslationAccessors;
use super::translation::TranslationUnit;
use super::util;

pub struct Parser {
	rules: Vec<Box<dyn Rule>>,
}

impl Parser {
	pub fn new() -> Self {
		Self {
			rules: super::rule::get_rule_registry().iter().collect::<Vec<_>>(),
		}
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
	/// defined first in the parser are prioritized. See [Parser::add_rule] for
	/// information on how to prioritize rules.
	///
	/// Notes that the result of every call to [`Rule::next_match`] gets stored
	/// in a table: [`ParserState::matches`]. Until the cursor steps over a
	/// position in the table, `next_match` won't be called.
	fn next_match(
		&self,
		unit: &mut TranslationUnit,
		cursor: &Cursor,
	) -> Option<(Cursor, &Box<dyn Rule>, Box<dyn Any>)> {
		let state = unit.scope().parser_state_mut();
		while state.matches.len() < self.rules.len() {
			state.matches.push((0, None));
		}

		self.rules
			.iter()
			.zip(state.matches.iter_mut())
			.for_each(|(rule, (pos, data))| {
				// Don't upate if not stepped over yet
				if *pos > cursor.pos {
					return;
				}

				// Update next match position
				(*pos, *data) = match rule.next_match(&state.mode, cursor) {
					None => (usize::MAX, None),
					Some((mut new_pos, mut new_data)) => {
						// Check if escaped
						while pos != usize::MAX {
							let content = cursor.source.content().as_str();
							let mut codepoints = content[0..new_pos].chars();
							let mut escaped = false;
							'inner: loop {
								let g = codepoints.next_back();
								if g.is_none() || g.unwrap() != "\\" {
									break 'inner;
								}

								escaped = !escaped;
							}

							// Find next potential match
							(new_pos, new_data) = match rule.next_match(&state.mode, cursor) {
								None => (usize::MAX, new_data), // Stop iterating
								Some((new_pos, new_data)) => (new_pos, new_data),
							};

							if !escaped {
								break;
							}
						}
						(new_pos, Some(new_data))
					}
				};
			});

		// Get winning match
		match state
			.matches
			.iter()
			.enumerate()
			.min_by_key(|(_, (pos, _))| pos)
			.map(|(winner, (pos, _))| (winner, *pos))
			.unwrap()
		{
			(_, usize::MAX) => None,
			(winner, pos) => state.matches[winner]
				.1
				.take()
				.map(|data| (cursor.at(pos), &self.rules[winner], data)),
		}
	}

	fn add_text<'u>(&'u self, unit: &mut TranslationUnit<'u>, range: Range<Cursor>) {
		let token: Token = range.into();
		let content = token.content().chars().fold(String::default(), {
			let mut escaped = false;
			|| {
				|mut s, c|
				{
					if c == '\\'
					{
						escaped = !escaped;
					}
					else if escaped
					{
						s.push(c);
						escaped = false;
					}
					else if c == '\n'
					{
						s.push(' ');
					}
					else
					{
						s.push(c);
					}
					s
				}()
			}
		});

		if content.is_empty() {
			return;
		}

		unit.add_content(Arc::new(Text::new(token, content.into())));
	}

	/// Parses the current scope in the translation unit
	pub fn parse<'u>(&'u self, unit: &mut TranslationUnit<'u>) -> Vec<Report> {
		let mut cursor: Cursor = unit.scope().source().into();
		let mut reports = Vec::default();

		while let Some((next_cursor, rule, rule_data)) = self.next_match(unit, &cursor) {
			// Unmatched content added as text
			self.add_text(unit, cursor..next_cursor);

			let (next_cursor, rule_reports) = rule.on_match(unit, &next_cursor, rule_data);
			
			cursor = next_cursor;
			reports.extend(rule_reports);
		}
		// Add leftover as text
		self.add_text(unit, cursor..cursor.at(cursor.source.content().len()));

		reports
	}
}

pub trait ParserRuleAccessor {
	fn iter_rules(&self) -> Iter<Box<dyn Rule>>;
}

impl ParserRuleAccessor for Parser {
    fn iter_rules(&self) -> Iter<Box<dyn Rule>> {
        self.rules.iter()
    }
}
