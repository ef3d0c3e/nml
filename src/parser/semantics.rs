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
	tokens: Vec<SemanticToken>,
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

	pub fn add(&mut self, range: Range<usize>, token_type: u32, token_modifier: u32)
	{
		let current = self.cursor.clone();
		self.cursor.move_to(range.start);

		let delta_line = self.cursor.line - current.line;
		let delta_start = if delta_line == 0
		{
			self.cursor.line_pos - current.line_pos
		} else { self.cursor.line_pos };

		self.tokens.push(SemanticToken{
			delta_line: delta_line as u32,
			delta_start: delta_start as u32,
			length: 10,
			token_type,
			token_modifiers_bitset: token_modifier,
		});
	}
}
