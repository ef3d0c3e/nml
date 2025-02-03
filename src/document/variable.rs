use super::document::Document;
use crate::elements::text::elem::Text;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use std::path::PathBuf;
use std::sync::Arc;

/// Internal name for variables
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VariableName(pub String);

impl core::fmt::Display for VariableName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

/// Trait for document variables
pub trait Variable {
	/// Gets the definition location of the variable
	fn location(&self) -> &Token;

	/// Gets the name of the variable
	fn name(&self) -> &VariableName;

	/// Converts variable to a string
	fn to_string(&self) -> String;

	/// The token when the variable value was defined from
	fn value_token(&self) -> &Token;

	/// Expands the variable when it is requested
	fn parse<'a>(&self, state: &ParserState, location: Token, document: &'a dyn Document<'a>);
}

impl core::fmt::Debug for dyn Variable {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{:#?}{{{}}}", self.name(), self.to_string())
	}
}

/// Base variables, a variable that is parsed when invoked
#[derive(Debug)]
pub struct BaseVariable {
	location: Token,
	name: VariableName,
	value_token: Token,
	value: String,
}

impl BaseVariable {
	pub fn new(location: Token, name: VariableName, value_token: Token, value: String) -> Self {
		Self {
			location,
			name,
			value_token,
			value,
		}
	}
}

impl Variable for BaseVariable {
	fn location(&self) -> &Token { &self.location }

	fn name(&self) -> &VariableName { &self.name }

	fn to_string(&self) -> String { self.value.clone() }

	fn value_token(&self) -> &Token { &self.value_token }

	fn parse<'a>(&self, state: &ParserState, _location: Token, document: &'a dyn Document<'a>) {
		let source = Arc::new(VirtualSource::new(
			self.location().clone(),
			format!(":VAR:{}", self.name()),
			self.to_string(),
		));

		state.with_state(|new_state| {
			let _ = new_state
				.parser
				.parse_into(new_state, source, document, ParseMode::default());
		});
	}
}

/// A path-aware variable, expanded as text when processed
#[derive(Debug)]
pub struct PathVariable {
	location: Token,
	name: VariableName,
	value_token: Token,
	path: PathBuf,
}

impl PathVariable {
	pub fn new(location: Token, name: VariableName, value_token: Token, path: PathBuf) -> Self {
		Self {
			location,
			name,
			value_token,
			path,
		}
	}
}

impl Variable for PathVariable {
	fn location(&self) -> &Token { &self.location }

	fn name(&self) -> &VariableName { &self.name }

	fn to_string(&self) -> String { self.path.to_str().unwrap().to_string() }

	fn value_token(&self) -> &Token { &self.value_token }

	fn parse(&self, state: &ParserState, location: Token, document: &dyn Document) {
		let source = Arc::new(VirtualSource::new(
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
