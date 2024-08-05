use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use ariadne::Report;

use crate::document::document::Document;
use crate::parser::source::Source;
use crate::parser::source::Token;

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
		state: &mut ParserState,
		document: &'a (dyn Document<'a> + 'a),
	) -> Vec<Report<(Rc<dyn Source>, Range<usize>)>>;
	fn on_end<'a>(
		&self,
		location: Token,
		state: &mut ParserState,
		document: &'a (dyn Document<'a> + 'a),
	) -> Vec<Report<(Rc<dyn Source>, Range<usize>)>>;
}

#[derive(Default)]
pub struct CustomStyleHolder {
	custom_styles: HashMap<String, Rc<dyn CustomStyle>>,
}

impl CustomStyleHolder {
	fn get(&self, style_name: &str) -> Option<Rc<dyn CustomStyle>> {
		self.custom_styles
			.get(style_name)
			.map(|style| style.clone())
	}

	fn insert(&mut self, style: Rc<dyn CustomStyle>) {
		self.custom_styles.insert(style.name().into(), style);
	}
}
