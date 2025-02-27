use std::rc::Rc;


use crate::parser::source::Token;

use super::element::Element;

/// Name for references
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Refname
{
	Internal(String),
	External(String, String),
	Bibliography(String, String)
}

impl core::fmt::Display for Refname {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self
		{
			Refname::Internal(a) => write!(f, "{a}"),
			Refname::External(a, b) => write!(f, "{a}#{b}"),
			Refname::Bibliography(a, b) => write!(f, "{a}@{b}"),
		}
    }
}

impl TryFrom<&str> for Refname
{
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
		if s.is_empty() {
			return Err("Refname cannot be empty".to_string());
		}

		// Validate
		let mut kind = None;
		s
			.chars()
			.try_for_each(|c| {
				if c == '#' || c == '@'
				{
					if kind.is_some()
					{
						return Err(format!(
								"Refname `{s}` cannot contain `{c}` after previous specifier"
						));
					}
					kind = Some(c);
				}
				else if c.is_ascii_punctuation() && !(c == '.' || c == '_') {
					return Err(format!(
							"Refname `{s}` cannot contain punctuation codepoint: `{c}`"
					));
				}
				else if c.is_whitespace() {
					return Err(format!(
							"Refname `{s}` cannot contain whitespaces: `{c}`"
					));
				}
				else if c.is_control() {
					return Err(format!(
							"Refname `{s}` cannot contain control codepoint: `{c}`"
					));
				}

				Ok(())
			})?;
		match kind {
			Some('#') => {
				let p = s.split_once('#')
					.map(|(a, b)| (a.to_string(), b.to_string()))
					.unwrap();
				Ok(Self::External(p.0, p.1))
			},
			Some('@') => {
				let p = s.split_once('@')
					.map(|(a, b)| (a.to_string(), b.to_string()))
					.unwrap();
				Ok(Self::Bibliography(p.0, p.1))
			},
			_ => Ok(Self::Internal(s.to_string())),
		}
    }
}

/// References available inside a document
#[derive(Debug)]
pub struct InternalReference {
	// Declaration 
	pub location: Token,
	/// Name of the reference
	pub refname: Refname,
}

impl InternalReference {
	pub fn name(&self) -> &Refname { &self.refname }
}
