use std::collections::HashMap;
use std::rc::Rc;

use crate::parser::source::Source;

use super::hints::HintsData;
use super::semantic::SemanticsData;
use super::semantic::Tokens;

#[derive(Debug)]
pub struct LSPData {
	pub semantic_tokens: Tokens,
	pub semantic_data: HashMap<Rc<dyn Source>, SemanticsData>,
	pub inlay_hints: HashMap<Rc<dyn Source>, HintsData>,
}

impl LSPData {
	pub fn new() -> Self {
		Self {
			semantic_tokens: Tokens::new(),
			semantic_data: HashMap::new(),
			inlay_hints: HashMap::new(),
		}
	}
}
