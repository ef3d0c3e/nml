use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::parser::parser::ParserState;
use crate::parser::reports::Report;
use crate::parser::source::Token;

use super::elem::Block;

/// The type of a block
pub trait BlockType: core::fmt::Debug {
	/// Name of the block
	fn name(&self) -> &'static str;

	/// Parses block properties
	fn parse_properties(
		&self,
		reports: &mut Vec<Report>,
		state: &ParserState,
		token: Token,
	) -> Option<Box<dyn Any>>;

	/// Compile block
	fn compile(
		&self,
		block: &Block,
		properties: &Box<dyn Any>,
		compiler: &Compiler,
		document: &dyn Document,
		cursor: usize,
	) -> Result<String, String>;
}

pub struct BlockHolder {
	blocks: HashMap<String, Rc<dyn BlockType>>,
}

macro_rules! create_blocks {
	( $($construct:expr),+ $(,)? ) => {{
		let mut map = HashMap::new();
		$(
			let val = Rc::new($construct) as Rc<dyn BlockType>;
			map.insert(val.name().to_string(), val);
		)+
		map
	}};
}

#[auto_registry::generate_registry(registry = "block_types", target = make_blocks, return_type = HashMap<String, Rc<dyn BlockType>>, maker = create_blocks)]
impl Default for BlockHolder {
	fn default() -> Self {
		Self {
			blocks: make_blocks(),
		}
	}
}

impl BlockHolder {
	pub fn get(&self, layout_name: &str) -> Option<Rc<dyn BlockType>> {
		self.blocks.get(layout_name).cloned()
	}

	pub fn insert(&mut self, block: Rc<dyn BlockType>) {
		self.blocks.insert(block.name().into(), block);
	}
}
