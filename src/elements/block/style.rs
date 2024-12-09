use serde::Deserialize;
use serde::Serialize;

use crate::impl_elementstyle;

pub static STYLE_KEY_QUOTE: &str = "style.block.quote";

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum AuthorPos {
	Before,
	After,
	None,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QuoteStyle {
	pub author_pos: AuthorPos,
	pub format: [String; 3],
}

impl Default for QuoteStyle {
	fn default() -> Self {
		Self {
			author_pos: AuthorPos::After,
			format: [
				"{author}, {cite}".into(),
				"{author}".into(),
				"{cite}".into(),
			],
		}
	}
}

impl_elementstyle!(QuoteStyle, STYLE_KEY_QUOTE);
