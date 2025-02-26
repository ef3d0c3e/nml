use std::rc::Rc;

use serde::Deserialize;
use serde::Serialize;

use crate::compiler::resolver::ErasedReference;
use crate::parser::source::Token;

/// Internal name for references
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

    fn try_from(value: &str) -> Result<Self, Self::Error> {
		let trimmed = value.trim_start().trim_end();
		if trimmed.is_empty() {
			return Err("Refname cannot be empty".to_string());
		}

		// Validate
		let mut kind = None;
		trimmed
			.chars()
			.try_for_each(|c| {
				if c == '#' || c == '@'
				{
					if kind.is_some()
					{
						return Err(format!(
								"Refname `{trimmed}` cannot contain `{c}` after previous specifier"
						));
					}
					kind = Some(c);
				}
				else if c.is_ascii_punctuation() && !(c == '.' || c == '_') {
					return Err(format!(
							"Refname `{trimmed}` cannot contain punctuation codepoint: `{c}`"
					));
				}
				else if c.is_whitespace() {
					return Err(format!(
							"Refname `{trimmed}` cannot contain whitespaces: `{c}`"
					));
				}
				else if c.is_control() {
					return Err(format!(
							"Refname `{trimmed}` cannot contain control codepoint: `{c}`"
					));
				}

				Ok(())
			})?;
		match kind {
			Some('#') => {
				let p = trimmed.split_once('#')
					.map(|(a, b)| (a.to_string(), b.to_string()))
					.unwrap();
				Ok(Self::External(p.0, p.1))
			},
			Some('@') => {
				let p = trimmed.split_once('@')
					.map(|(a, b)| (a.to_string(), b.to_string()))
					.unwrap();
				Ok(Self::Bibliography(p.0, p.1))
			},
			_ => Ok(Self::Internal(trimmed.to_string())),
		}
    }
}

// Declared reference
#[derive(Debug)]
pub struct Reference {
	location: Token,
	/// Name of the reference
	refname: Refname,
	/// Internal path to the reffered element
	internal_path: Vec<usize>,
}

impl Reference {
	pub fn name(&self) -> &Refname { &self.refname }
}
