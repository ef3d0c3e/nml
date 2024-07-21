use std::rc::Rc;

use crate::parser::source::{Cursor, Source};

#[derive(Debug, Clone)]
pub struct LineCursor
{
	pub pos: usize,
	pub line: usize,
	pub line_pos: usize,
	pub source: Rc<dyn Source>,
}

impl LineCursor
{
	/// Creates [`LineCursor`] at position
	///
	/// # Error
	/// This function will panic if [`pos`] is not utf8 aligned
	///
	/// Note: this is a convenience function, it should be used
	/// with parsimony as it is expensive
	pub fn at(&mut self, pos: usize)
	{
		if pos > self.pos
		{
			let start = self.pos;
			//eprintln!("slice{{{}}}, want={pos}", &self.source.content().as_str()[start..pos]);
			let mut it = self.source.content()
				.as_str()[start..] // pos+1
				.chars()
				.peekable();

			let mut prev = self.source.content()
					.as_str()[..start+1]
					.chars()
					.rev()
					.next();
			//eprintln!("prev={prev:#?}");
			while self.pos < pos
			{
				let c = it.next().unwrap();
				let len = c.len_utf8();

				self.pos += len;
				if prev == Some('\n')
				{
					self.line += 1;
					self.line_pos = 0;
				}
				else
				{
					self.line_pos += len;
				}

				//eprintln!("({}, {c:#?}) ({} {})", self.pos, self.line, self.line_pos);
				prev = Some(c);
			}

			/*
			self.source.content()
				.as_str()[start..pos+1]
				.char_indices()
				.for_each(|(at, c)| {
					self.pos = at+start;

					if c == '\n'
					{
						self.line += 1;
						self.line_pos = 0;
					}
					else
					{
						self.line_pos += c.len_utf8();
					}

				});
			*/
		}
		else if pos < self.pos
		{
			todo!("");
			self.source.content()
				.as_str()[pos..self.pos]
				.char_indices()
				.rev()
				.for_each(|(len, c)| {
					self.pos -= len;
					if c == '\n'
					{
						self.line -= 1;
					}
				});
			self.line_pos = self.source.content()
				.as_str()[..self.pos]
				.char_indices()
				.rev()
				.find(|(_, c)| *c == '\n')
				.map(|(line_start, _)| self.pos-line_start)
				.unwrap_or(0);
		}

		// May fail if pos is not utf8-aligned
		assert_eq!(pos, self.pos);
	}
}

impl From<&LineCursor> for Cursor
{
    fn from(value: &LineCursor) -> Self {
		Self {
			pos: value.pos,
			source: value.source.clone()
		}
    }
}
