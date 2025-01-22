use serde::Deserialize;
use serde::Serialize;

use super::document::Document;

/// Validates the name of a reference, returning an error message in case the name is invalid
///
/// # Notes
///
/// A valid reference name must not be empty and cannot contain the following:
///  - Ascii punctuation outside of `.` and `_`. This is imposed in order to avoid confusion when
///  passing a reference as a property, as properties are often delimited by `[]` or `:`
///  - white spaces, e.g spaces, tabs or `\n`
///  - no special ascii characters (no control sequences)
pub fn validate_refname<'a>(
	document: &dyn Document,
	name: &'a str,
	check_duplicate: bool,
) -> Result<&'a str, String> {
	let trimmed = name.trim_start().trim_end();
	if trimmed.is_empty() {
		return Err("Refname cannot be empty".to_string());
	}

	for c in trimmed.chars() {
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
	}

	if check_duplicate && document.get_reference(trimmed).is_some() {
		Err(format!("Refname `{trimmed}` is already in use!"))
	} else {
		Ok(trimmed)
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
