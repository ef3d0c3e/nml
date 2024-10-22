use super::document::Document;
use crate::elements::text::Text;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use std::path::PathBuf;
use std::rc::Rc;

pub trait Variable {
	fn location(&self) -> &Token;

	fn name(&self) -> &str;

	/// Converts variable to a string
	fn to_string(&self) -> String;

	fn parse<'a>(&self, state: &ParserState, location: Token, document: &'a dyn Document<'a>);
}

impl core::fmt::Debug for dyn Variable {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}{{{}}}", self.name(), self.to_string())
	}
}

#[derive(Debug)]
pub struct BaseVariable {
	location: Token,
	name: String,
	value: String,
}

impl BaseVariable {
	pub fn new(location: Token, name: String, value: String) -> Self {
		Self {
			location,
			name,
			value,
		}
	}
}

impl Variable for BaseVariable {
	fn location(&self) -> &Token { &self.location }

	fn name(&self) -> &str { self.name.as_str() }

	fn to_string(&self) -> String { self.value.clone() }

	fn parse<'a>(&self, state: &ParserState, _location: Token, document: &'a dyn Document<'a>) {
		let source = Rc::new(VirtualSource::new(
			self.location().clone(),
			self.name().to_string(),
			self.to_string(),
		));

		state.with_state(|new_state| {
			let _ = new_state.parser.parse_into(new_state, source, document, ParseMode::default());
		});
	}
}

#[derive(Debug)]
pub struct PathVariable {
	location: Token,
	name: String,
	path: PathBuf,
}

impl PathVariable {
	pub fn new(location: Token, name: String, path: PathBuf) -> Self {
		Self {
			location,
			name,
			path,
		}
	}
}

impl Variable for PathVariable {
	fn location(&self) -> &Token { &self.location }

	fn name(&self) -> &str { self.name.as_str() }

	fn to_string(&self) -> String { self.path.to_str().unwrap().to_string() }

	fn parse(&self, state: &ParserState, location: Token, document: &dyn Document) {
		let source = Rc::new(VirtualSource::new(
			location,
			self.name().to_string(),
			self.to_string(),
		));

		state.push(
			document,
			Box::new(Text::new(
				Token::new(0..source.content().len(), source),
				self.to_string(),
			)),
		);
	}
}
