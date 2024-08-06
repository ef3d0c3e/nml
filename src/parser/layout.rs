use std::any::Any;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::elements::layout::LayoutToken;

/// Represents the type of a layout
pub trait LayoutType: core::fmt::Debug {
	/// Name of the layout
	fn name(&self) -> &'static str;

	/// Parses layout properties
	fn parse_properties(&self, properties: &str) -> Result<Option<Box<dyn Any>>, String>;

	/// Expected number of blocks
	fn expects(&self) -> Range<usize>;

	/// Compile layout
	fn compile(
		&self,
		token: LayoutToken,
		id: usize,
		properties: &Option<Box<dyn Any>>,
		compiler: &Compiler,
		document: &dyn Document,
	) -> Result<String, String>;
}

#[derive(Default)]
pub struct LayoutHolder {
	layouts: HashMap<String, Rc<dyn LayoutType>>,
}

impl LayoutHolder {
	pub fn get(&self, layout_name: &str) -> Option<Rc<dyn LayoutType>> {
		self.layouts.get(layout_name).map(|layout| layout.clone())
	}

	pub fn insert(&mut self, layout: Rc<dyn LayoutType>) {
		self.layouts.insert(layout.name().into(), layout);
	}
}
