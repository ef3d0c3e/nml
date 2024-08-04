use std::cell::Ref;
use std::cell::RefMut;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use ariadne::Report;

use crate::parser::parser::Parser;
use crate::parser::source::Source;
use crate::parser::source::Token;

use super::document::Document;

#[derive(Debug, PartialEq, Eq)]
pub enum CustomStyleToken {
	Toggle(String),
	Pair(String, String),
}

pub trait CustomStyle: core::fmt::Debug {
	/// Name for the custom style
	fn name(&self) -> &str;
	/// Gets the begin and end token for a custom style
	fn tokens(&self) -> &CustomStyleToken;

	fn on_start<'a>(
		&self,
		location: Token,
		parser: &dyn Parser,
		document: &'a (dyn Document<'a> + 'a),
	) -> Result<(), Report<(Rc<dyn Source>, Range<usize>)>>;
	fn on_end<'a>(
		&self,
		location: Token,
		parser: &dyn Parser,
		document: &'a (dyn Document<'a> + 'a),
	) -> Result<(), Report<(Rc<dyn Source>, Range<usize>)>>;
}

pub trait CustomStyleHolder {
	/// gets a reference to all defined custom styles
	fn custom_styles(&self) -> Ref<'_, HashMap<String, Rc<dyn CustomStyle>>>;

	/// gets a (mutable) reference to all defined custom styles
	fn custom_styles_mut(&self) -> RefMut<'_, HashMap<String, Rc<dyn CustomStyle>>>;

	fn get_custom_style(&self, style_name: &str) -> Option<Rc<dyn CustomStyle>> {
		self.custom_styles()
			.get(style_name)
			.map(|style| style.clone())
	}

	fn insert_custom_style(&self, style: Rc<dyn CustomStyle>) {
		self.custom_styles_mut().insert(style.name().into(), style);
	}
}
