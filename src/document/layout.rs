use std::any::Any;
use std::cell::Ref;
use std::cell::RefMut;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::elements::layout::LayoutToken;

use super::document::Document;

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

pub trait LayoutHolder {
	/// gets a reference to all defined layouts
	fn layouts(&self) -> Ref<'_, HashMap<String, Rc<dyn LayoutType>>>;

	/// gets a (mutable) reference to all defined layours
	fn layouts_mut(&self) -> RefMut<'_, HashMap<String, Rc<dyn LayoutType>>>;

	fn get_layout(&self, layout_name: &str) -> Option<Rc<dyn LayoutType>> {
		self.layouts().get(layout_name).map(|layout| layout.clone())
	}

	fn insert_layout(&self, layout: Rc<dyn LayoutType>) {
		self.layouts_mut().insert(layout.name().into(), layout);
	}
}
