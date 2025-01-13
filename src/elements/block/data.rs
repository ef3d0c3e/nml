use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;

use crate::compiler::compiler::{Compiler, CompilerOutput};
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
	fn compile<'e>(
		&'e self,
		block: &'e Block,
		properties: &'e Box<dyn Any>,
		compiler: &'e Compiler,
		document: &'e dyn Document,
		output: &'e mut CompilerOutput<'e>,
	) -> Result<&mut CompilerOutput<'e>, Vec<Report>>;
}

/// Holds all registered [`BlockType`]
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
	/// Gets a [`BlockType`] by name
	pub fn get(&self, layout_name: &str) -> Option<Rc<dyn BlockType>> {
		self.blocks.get(layout_name).cloned()
	}

	/// Inserts a new block
	///
	/// # Error
	///
	/// If a block by the same name already exists, an error is returned
	pub fn insert(&mut self, block: Rc<dyn BlockType>) -> Result<(), String> {
		if let Some(previous) = self.blocks.insert(block.name().into(), block) {
			Err(format!("Duplicate block types: `{}`", previous.name()))
		} else {
			Ok(())
		}
	}
}
