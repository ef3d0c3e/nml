use super::document::Document;

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
	use std::rc::Rc;

	use super::*;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::ParseMode;
	use crate::parser::parser::Parser;
	use crate::parser::parser::ParserState;
	use crate::parser::source::SourceFile;

	#[test]
	fn validate_refname_tests() {
		let source = Rc::new(SourceFile::with_content(
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
