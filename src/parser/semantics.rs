use std::{ops::Range, rc::Rc};

use tower_lsp::lsp_types::SemanticToken;

use super::source::{LineCursor, Source};


/// Semantics for a buffer
#[derive(Debug)]
pub struct Semantics
{
	/// The current cursor
	cursor: LineCursor,

	/// Semantic tokens
	pub tokens: Vec<SemanticToken>,
}

impl Semantics
{
	pub fn new(source: Rc<dyn Source>) -> Semantics
	{
		Self {
			cursor: LineCursor::new(source),
			tokens: vec![]
		}
	}

	pub fn add(&mut self, source: Rc<dyn Source>, range: Range<usize>, token_type: u32, token_modifier: u32)
	{
		let mut current = self.cursor.clone();
		self.cursor.move_to(range.start);

		while self.cursor.pos != range.end
		{
			let end = source.content()[self.cursor.pos..].find('\n')
				.unwrap_or(source.content().len() - self.cursor.pos);
			let len = usize::min(range.end - self.cursor.pos, end);

			let delta_line = self.cursor.line - current.line;
			let delta_start = if delta_line == 0
			{
				if let Some(last) = self.tokens.last()
				{
					self.cursor.line_pos - current.line_pos + last.length as usize
				}
				else
				{
					self.cursor.line_pos - current.line_pos
				}
			} else { self.cursor.line_pos };

			eprintln!("CURRENT={:#?}, CURS={:#?}", current, self.cursor);
			self.tokens.push(SemanticToken{
				delta_line: delta_line as u32,
				delta_start: delta_start as u32,
				length: len as u32,
				token_type,
				token_modifiers_bitset: token_modifier,
			});
			current = self.cursor.clone();
			self.cursor.move_to(self.cursor.pos + len);
		}
	}
}
