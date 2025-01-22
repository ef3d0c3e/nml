use std::any::Any;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::document::document::Document;
use crate::parser::parser::ParserState;
use crate::parser::reports::Report;
use crate::parser::source::Token;

use super::custom::LayoutToken;

/// Represents the type of a layout
pub trait LayoutType: core::fmt::Debug {
	/// Name of the layout
	fn name(&self) -> &'static str;

	/// Parses layout properties
	fn parse_properties(
		&self,
		reports: &mut Vec<Report>,
		state: &ParserState,
		token: Token,
	) -> Option<Box<dyn Any>>;

	/// Expected number of blocks
	fn expects(&self) -> Range<usize>;

	/// Compile layout
	fn compile<'e>(
		&'e self,
		token: LayoutToken,
		id: usize,
		properties: &'e Box<dyn Any>,
		compiler: &'e Compiler,
		document: &'e dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>>;
}

pub struct LayoutHolder {
	layouts: HashMap<String, Rc<dyn LayoutType>>,
}

macro_rules! create_layouts {
	( $($construct:expr),+ $(,)? ) => {{
		let mut map = HashMap::new();
		$(
			let val = Rc::new($construct) as Rc<dyn LayoutType>;
			map.insert(val.name().to_string(), val);
		)+
		map
	}};
}

#[auto_registry::generate_registry(registry = "layouts", target = make_layouts, return_type = HashMap<String, Rc<dyn LayoutType>>, maker = create_layouts)]
impl Default for LayoutHolder {
	fn default() -> Self {
		Self {
			layouts: make_layouts(),
		}
	}
}

impl LayoutHolder {
	pub fn get(&self, layout_name: &str) -> Option<Rc<dyn LayoutType>> {
		self.layouts.get(layout_name).cloned()
	}

	pub fn insert(&mut self, layout: Rc<dyn LayoutType>) {
		self.layouts.insert(layout.name().into(), layout);
	}
}
