use std::collections::HashMap;
use std::rc::Rc;
use std::ops::Deref;

use crate::document::document::Document;
use crate::parser::source::Token;

use crate::parser::reports::*;


use super::parser::ParserState;

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
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
	) -> Vec<Report>;
	fn on_end<'a>(
		&self,
		location: Token,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
	) -> Vec<Report>;
}

#[derive(Default)]
pub struct CustomStyleHolder {
	custom_styles: HashMap<String, Rc<dyn CustomStyle>>,
}

impl CustomStyleHolder {
	pub fn get(&self, style_name: &str) -> Option<Rc<dyn CustomStyle>> {
		self.custom_styles
			.get(style_name).cloned()
	}

	pub fn insert(&mut self, style: Rc<dyn CustomStyle>) {
		self.custom_styles.insert(style.name().into(), style);
	}
}

impl Deref for CustomStyleHolder {
    type Target = HashMap<String, Rc<dyn CustomStyle>>;

    fn deref(&self) -> &Self::Target {
        &self.custom_styles
    }
}
