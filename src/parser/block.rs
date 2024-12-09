use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::elements::block::Block;

use super::parser::ParserState;
use super::reports::Report;
use super::source::Token;

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

#[derive(Default)]
pub struct BlockHolder {
	blocks: HashMap<String, Rc<dyn BlockType>>,
}

impl BlockHolder {
	pub fn get(&self, layout_name: &str) -> Option<Rc<dyn BlockType>> {
		self.blocks.get(layout_name).cloned()
	}

	pub fn insert(&mut self, block: Rc<dyn BlockType>) {
		self.blocks.insert(block.name().into(), block);
	}
}

