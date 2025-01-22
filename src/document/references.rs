use std::rc::Rc;

use serde::Deserialize;
use serde::Serialize;

use crate::parser::source::Source;
use crate::parser::translation::Scope;

use super::document::Document;

/// A reference inside a document
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Refname(String);

impl core::fmt::Display for Refname {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl Refname {
	pub fn from_str<S: AsRef<str>>(str: S) -> Result<Self, String> {
		let trimmed = str.as_ref().trim_start().trim_end();
		if trimmed.is_empty() {
			return Err("Refname cannot be empty".to_string());
		}

		trimmed
			.chars()
			.try_for_each(|c| {
				if c.is_ascii_punctuation() && !(c == '.' || c == '_') {
					return Err(format!(
							"Refname `{trimmed}` cannot contain punctuation codepoint: `{c}`"
					));
				}
				if c.is_whitespace() {
					return Err(format!(
							"Refname `{trimmed}` cannot contain whitespaces: `{c}`"
					));
				}
				if c.is_control() {
					return Err(format!(
							"Refname `{trimmed}` cannot contain control codepoint: `{c}`"
					));
				}

				Ok(())
			})?;

		Ok(Self(trimmed.to_string()))
	}
}

#[derive(Debug)]
pub struct Reference {
	refname: Refname,
	/// Internal path to the reffered element
	internal_path: Vec<usize>,
	/// External path to the reffered element
	external_path: String,
}

impl Reference {
	pub fn name(&self) -> &Refname { &self.refname }
}

/// References inside the current document
///
/// # Note
///
/// It is only possible to reference an element nested by at most 1 level.
/// For instance, it is possible to reference an element inside a `Block`. But not if the block is
/// inside another `Block`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ElemReference {
	Direct(usize),

	// Reference nested inside another element, e.g [`Paragraph`] or [`Media`]
	Nested(usize, usize),
}

/// A reference that points to another document. Either the other document is specified by name or
/// unspecified -- in which case all documents are searched for the reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrossReference {
	/// When the referenced document is unspecified
	Unspecific(String),

	/// When the referenced document is specified
	Specific(String, String),
}

impl core::fmt::Display for CrossReference {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			CrossReference::Unspecific(name) => write!(f, "#{name}"),
			CrossReference::Specific(doc_name, name) => write!(f, "{doc_name}#{name}"),
		}
	}
}

#[cfg(test)]
pub mod tests {

	use super::*;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::ParseMode;
	use crate::parser::parser::Parser;
	use crate::parser::parser::ParserState;
	use crate::parser::source::SourceFile;
	use std::sync::Arc;

	#[test]
	fn validate_refname_tests() {
		let source = Arc::new(SourceFile::with_content(
			"".to_string(),
			"#{ref} Section".to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(
			ParserState::new(&parser, None),
			source,
			None,
			ParseMode::default(),
		);

		assert_eq!(validate_refname(&*doc, " abc ", true), Ok("abc"));
		assert_eq!(
			validate_refname(&*doc, " 	 	Some_reference  		 ", true),
			Ok("Some_reference")
		);
		assert!(validate_refname(&*doc, "", true).is_err());
		assert!(validate_refname(&*doc, "\n", true).is_err());
		assert!(validate_refname(&*doc, "'", true).is_err());
		assert!(validate_refname(&*doc, "]", true).is_err());

		// Duplicate
		assert!(validate_refname(&*doc, "ref", true).is_err());
	}
}
