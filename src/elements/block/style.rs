use serde::Deserialize;
use serde::Serialize;

use crate::impl_elementstyle;

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum AuthorPos {
	Before,
	After,
	None,
}

#[derive(Debug, Serialize, Deserialize)]
#[auto_registry::auto_registry(registry = "elem_styles")]
pub struct QuoteStyle {
	pub author_pos: AuthorPos,
	pub format: [String; 3],
}
impl_elementstyle!(QuoteStyle, "style.block.quote");

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
