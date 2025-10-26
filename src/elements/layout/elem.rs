use std::sync::Arc;

use crate::elements::layout::state::Layout;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct LayoutElem {
	pub(crate) location: Token,
	pub(crate) id: usize,
	pub(crate) layout: Arc<dyn Layout + Send + Sync>,
}
