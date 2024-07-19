use std::{path::PathBuf, rc::Rc};
use crate::parser::{parser::Parser, source::{Source, Token, VirtualSource}};
use super::{document::Document, element::Text};


// TODO enforce to_string(from_string(to_string())) == to_string()
pub trait Variable
{
	fn location(&self) -> &Token;

	fn name(&self) -> &str;
	/// Parse variable from string, returns an error message on failure
	fn from_string(&mut self, str: &str) -> Option<String>;

	/// Converts variable to a string
	fn to_string(&self) -> String;

    fn parse<'a>(&self, location: Token, parser: &dyn Parser, document: &'a Document);
}

impl core::fmt::Debug for dyn Variable
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}{{{}}}", self.name(), self.to_string())
    }
}

#[derive(Debug)]
pub struct BaseVariable
{
	location: Token,
    name: String,
    value: String,
}

impl BaseVariable {
    pub fn new(location: Token, name: String, value: String) -> Self {
        Self { location, name, value }
    }
}

impl Variable for BaseVariable
{
	fn location(&self) -> &Token { &self.location }

    fn name(&self) -> &str { self.name.as_str() }

    fn from_string(&mut self, str: &str) -> Option<String> {
        self.value = str.to_string();
        None
    }

    fn to_string(&self) -> String { self.value.clone() }

    fn parse<'a>(&self, _location: Token, parser: &dyn Parser, document: &'a Document) {
		let source = Rc::new(VirtualSource::new(
				self.location().clone(),
			self.name().to_string(),
			self.to_string()));

		parser.parse_into(source, document);
	}
}

#[derive(Debug)]
pub struct PathVariable
{
	location: Token,
    name: String,
    path: PathBuf,
}

impl PathVariable
{
    pub fn new(location: Token, name: String, path: PathBuf) -> Self {
        Self { location, name, path }
    }
}

impl Variable for PathVariable
{
	fn location(&self) -> &Token { &self.location }

    fn name(&self) -> &str { self.name.as_str() }

    fn from_string(&mut self, str: &str) -> Option<String> {
        self.path = PathBuf::from(std::fs::canonicalize(str).unwrap());
        None
    }

    fn to_string(&self) -> String { self.path.to_str().unwrap().to_string() }

    fn parse<'a>(&self, location: Token, parser: &dyn Parser, document: &'a Document){
		// TODO: Avoid copying the location twice...
		// Maybe create a special VirtualSource where the `content()` method
		// calls `Variable::to_string()`
		let source = Rc::new(VirtualSource::new(
			location.clone(),
			self.name().to_string(),
			self.to_string()));

        parser.push(document, Box::new(Text::new(
			Token::new(0..source.content().len(), source),
			self.to_string()
        )));
    }
}

/*
struct ConfigVariable<T>
{
	value: T,
	name: String,

	desc: String,
	validator: Box<dyn Fn(&Self, &T) -> Option<&String>>,
}

impl<T> ConfigVariable<T>
{
	fn description(&self) -> &String { &self.desc }
}

impl<T> Variable for ConfigVariable<T>
where T: FromStr + Display
{
	fn name(&self) -> &str { self.name.as_str() }

	/// Parse variable from string, returns an error message on failure
	fn from_string(&mut self, str: &str) -> Option<String> {
		match str.parse::<T>()
		{
			Ok(value) => {
				(self.validator)(self, &value).or_else(|| {
                    self.value = value;
                    None
                })
			},
			Err(_) => return Some(format!("Unable to parse `{str}` into variable `{}`", self.name))
		}
	}

	/// Converts variable to a string
	fn to_string(&self) -> String { self.value.to_string() }
}
*/
