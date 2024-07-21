use std::any::Any;
use std::cell::{Ref, RefMut};
use std::rc::Rc;
use unicode_segmentation::UnicodeSegmentation;

use super::rule::Rule;
use super::source::{Cursor, Source};
use super::state::StateHolder;
use crate::document::document::Document;
use crate::document::element::Element;
use ariadne::Color;
use crate::lua::kernel::KernelHolder;

#[derive(Debug)]
pub struct ReportColors
{
	pub error: Color,
	pub warning: Color,
	pub info: Color,
	pub highlight: Color,
}

impl ReportColors {
	pub fn with_colors() -> Self {
		Self {
			error: Color::Red,
			warning: Color::Yellow,
			info: Color::BrightBlue,
			highlight: Color::BrightMagenta,
		}
	}

	pub fn without_colors() -> Self {
		Self {
			error: Color::Primary,
			warning: Color::Primary,
			info: Color::Primary,
			highlight: Color::Primary,
		}
	}
}

pub trait Parser: KernelHolder
{
	/// Gets the colors for formatting errors
	///
	/// When colors are disabled, all colors should resolve to empty string
	fn colors(&self) -> &ReportColors;

	fn rules(&self) -> &Vec<Box<dyn Rule>>;
	fn add_rule(&mut self, rule: Box<dyn Rule>, after: Option<&'static str>);

	fn state(&self) -> Ref<'_, StateHolder>;
	fn state_mut(&self) -> RefMut<'_, StateHolder>;

	// Update [`matches`] and returns the position of the next matched rule.
	// If rule is empty, it means that there are no rules left to parse (i.e
	// end of document).
	fn update_matches(&self, cursor: &Cursor, matches: &mut Vec<(usize, Option<Box<dyn Any>>)>)
		-> (Cursor, Option<&Box<dyn Rule>>, Option<Box<dyn Any>>) 
	{
		// Update matches
		// TODO: Trivially parellalizable
		self.rules().iter().zip(matches.iter_mut()).for_each(
			|(rule, (matched_at, match_data))| {
			// Don't upate if not stepped over yet
			if *matched_at > cursor.pos { return }

			(*matched_at, *match_data) = match rule.next_match(cursor)
			{
				None => (usize::MAX, None),
				Some((mut pos, mut data)) =>
				{
					// Check if escaped
					while pos != usize::MAX
					{
						let content = cursor.source.content().as_str();
						let mut graphemes = content[0 .. pos].graphemes(true);
						let mut escaped = false;
						'inner: loop
						{
							let g = graphemes.next_back();
							if !g.is_some() || g.unwrap() != "\\" { break 'inner; }

							escaped = !escaped;
						}
						if !escaped { break; }

						// Find next potential match
						(pos, data) = match rule.next_match(&cursor.at(pos+1)) {
							Some((new_pos, new_data)) => (new_pos, new_data),
							None => (usize::MAX, data) // Stop iterating
						}
					}

					(pos, (pos != usize::MAX).then_some(data))
				}
			}
		});

		// Get winning match
		let (winner, (next_pos, _match_data)) = matches.iter()
			.enumerate()
			.min_by_key(|(_, (pos, _match_data))| pos).unwrap();
		if *next_pos == usize::MAX // No rule has matched
		{
			let content = cursor.source.content();
			// No winners, i.e no matches left
			return (cursor.at(content.len()), None, None);
		}

		(cursor.at(*next_pos),
		Some(&self.rules()[winner]),
		std::mem::replace(&mut matches[winner].1, None))
	}

	/// Add an [`Element`] to the [`Document`]
	fn push<'a>(&self, doc: &'a Document<'a>, elem: Box<dyn Element>);

	/// Parse [`Source`] into a new [`Document`]
	fn parse<'a>(&self, source: Rc<dyn Source>, parent: Option<&'a Document<'a>>) -> Document<'a>;

	/// Parse [`Source`] into an already existing [`Document`]
	fn parse_into<'a>(&self, source: Rc<dyn Source>, document: &'a Document<'a>);
}
