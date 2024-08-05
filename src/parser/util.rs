use std::collections::HashMap;
use std::rc::Rc;

use unicode_segmentation::UnicodeSegmentation;

use crate::document::document::Document;
use crate::document::document::DocumentAccessors;
use crate::document::element::ElemKind;
use crate::elements::paragraph::Paragraph;

use super::parser::ParserState;
use super::source::Source;

/// Processes text for escape characters and paragraphing
pub fn process_text(document: &dyn Document, content: &str) -> String {
	let mut escaped = false;
	let mut newlines = 0usize; // Consecutive newlines
							//println!("Processing: [{content}]");
	let processed = content
		.graphemes(true)
		.fold((String::new(), None), |(mut out, prev), g| {
			if newlines != 0 && g != "\n" {
				newlines = 0;

				// Add a whitespace if necessary
				match out.chars().last() {
					Some(c) => {
						// NOTE: \n is considered whitespace, so previous codepoint can be \n
						// (Which can only be done by escaping it)
						if !c.is_whitespace() || c == '\n' {
							out += " ";
						}
					}
					None => {
						if document
							.last_element::<Paragraph>()
							.and_then(|par| {
								par.find_back(|e| e.kind() != ElemKind::Invisible)
									.and_then(|e| Some(e.kind() == ElemKind::Inline))
							})
							.unwrap_or(false)
						{
							out += " ";
						}
					} // Don't output anything
				}
			}

			// Output grapheme literally when escaped
			if escaped {
				escaped = false;
				return (out + g, Some(g));
			}
			// Increment newlines counter
			else if g == "\n" {
				newlines += 1;
				return (out, Some(g));
			}
			// Determine if escaped
			else if g == "\\" {
				escaped = !escaped;
				return (out, Some(g));
			}
			// Whitespaces
			else if g.chars().count() == 1 && g.chars().last().unwrap().is_whitespace() {
				// Content begins with whitespace
				if prev.is_none() {
					if document.last_element::<Paragraph>().is_some() {
						return (out + g, Some(g));
					} else {
						return (out, Some(g));
					}
				}
				// Consecutive whitespaces are converted to a single whitespace
				else if prev.unwrap().chars().count() == 1
					&& prev.unwrap().chars().last().unwrap().is_whitespace()
				{
					return (out, Some(g));
				}
			}

			return (out + g, Some(g));
		})
		.0
		.to_string();

	return processed;
}

/// Processed a string and escapes a single token out of it
/// Escaped characters other than the [`token`] will be not be treated as escaped
///
/// # Example
/// ```
/// assert_eq!(process_escaped('\\', "%", "escaped: \\%, also escaped: \\\\\\%, untouched: \\a"),
/// "escaped: %, also escaped: \\%, untouched: \\a");
/// ```
pub fn process_escaped<S: AsRef<str>>(escape: char, token: &'static str, content: S) -> String {
	let mut processed = String::new();
	let mut escaped = 0;
	let mut token_it = token.chars().peekable();
	for c in content
		.as_ref()
		.chars()
		.as_str()
		.trim_start()
		.trim_end()
		.chars()
	{
		if c == escape {
			escaped += 1;
		} else if escaped % 2 == 1 && token_it.peek().map_or(false, |p| *p == c) {
			let _ = token_it.next();
			if token_it.peek() == None {
				(0..(escaped / 2)).for_each(|_| processed.push(escape));
				escaped = 0;
				token_it = token.chars().peekable();
				processed.push_str(token);
			}
		} else {
			if escaped != 0 {
				// Add untouched escapes
				(0..escaped).for_each(|_| processed.push('\\'));
				token_it = token.chars().peekable();
				escaped = 0;
			}
			processed.push(c);
		}
	}
	// Add trailing escapes
	(0..escaped / 2).for_each(|_| processed.push('\\'));

	processed
}

/// Parses source into a single paragraph
/// If source contains anything but a single paragraph, an error is returned
pub fn parse_paragraph<'a>(
	state: &ParserState,
	source: Rc<dyn Source>,
	document: &'a dyn Document<'a>,
) -> Result<Box<Paragraph>, &'static str> {
	let parsed = state.with_state(|new_state| -> Box<dyn Document> {
		new_state.parser.parse(new_state, source.clone(), Some(document))
	});
	if parsed.content().borrow().len() > 1 {
		return Err("Parsed document contains more than a single paragraph");
	} else if parsed.content().borrow().len() == 0 {
		return Err("Parsed document is empty");
	} else if parsed.last_element::<Paragraph>().is_none() {
		return Err("Parsed element is not a paragraph");
	} else if state.parser.has_error() {
		// FIXME: If parser had an error before, this wold trigger
		return Err("Parser error");
	}

	let paragraph = parsed.content().borrow_mut().pop().unwrap();
	Ok(paragraph.downcast::<Paragraph>().unwrap())
}

#[derive(Debug)]
pub struct Property {
	required: bool,
	description: String,
	default: Option<String>,
}

impl Property {
	pub fn new(required: bool, description: String, default: Option<String>) -> Self {
		Self {
			required,
			description,
			default,
		}
	}
}

impl core::fmt::Display for Property {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self.default.as_ref() {
			None => write!(
				f,
				"{} {}",
				["[Opt]", "[Req]"][self.required as usize],
				self.description
			),
			Some(default) => write!(
				f,
				"{} {} (Deafult: {})",
				["[Opt]", "[Req]"][self.required as usize],
				self.description,
				default
			),
		}
	}
}

#[derive(Debug)]
pub enum PropertyMapError<E> {
	ParseError(E),
	NotFoundError(String),
}

#[derive(Debug)]
pub struct PropertyMap<'a> {
	pub(crate) properties: HashMap<String, (&'a Property, String)>,
}

impl<'a> PropertyMap<'a> {
	pub fn new() -> Self {
		Self {
			properties: HashMap::new(),
		}
	}

	pub fn get<T, Error, F: FnOnce(&'a Property, &String) -> Result<T, Error>>(
		&self,
		name: &str,
		f: F,
	) -> Result<(&'a Property, T), PropertyMapError<Error>> {
		let (prop, value) = match self.properties.get(name) {
			Some(found) => found,
			None => {
				return Err(PropertyMapError::NotFoundError(format!(
					"Property `{name}` not found"
				)))
			}
		};

		match f(prop, value) {
			Ok(parsed) => Ok((*prop, parsed)),
			Err(err) => Err(PropertyMapError::ParseError(err)),
		}
	}
}

#[derive(Debug)]
pub struct PropertyParser {
	pub properties: HashMap<String, Property>,
}

impl PropertyParser {
	/// Attempts to build a default propertymap
	///
	/// Returns an error if at least one [`Property`] is required and doesn't provide a default
	pub fn default(&self) -> Result<PropertyMap<'_>, String> {
		let mut properties = PropertyMap::new();

		for (name, prop) in &self.properties {
			match (prop.required, prop.default.as_ref()) {
				(true, None) => return Err(format!("Missing property `{name}` {prop}")),
				(false, None) => {}
				(_, Some(default)) => {
					properties
						.properties
						.insert(name.clone(), (prop, default.clone()));
				}
			}
		}

		Ok(properties)
	}

	/// Parses properties string "prop1=value1, prop2 = val\,2" -> {prop1: value1, prop2: val,2}
	///
	/// # Key-value pair
	///
	/// Property names/values are separated by a single '=' that cannot be escaped.
	/// Therefore names cannot contain the '=' character.
	///
	/// # Example
	///
	/// ```
	/// let mut properties = HashMap::new();
	/// properties.insert("width".to_string(),
	/// 	Property::new(true, "Width of the element in em".to_string(), None));
	///
	/// let parser = PropertyParser { properties };
	/// let pm = parser.parse("width=15").unwrap();
	///
	/// assert_eq!(pm.get("width", |_, s| s.parse::<i32>()).unwrap().1, 15);
	/// ```
	/// # Return value
	///
	/// Returns the parsed property map, or an error if either:
	///  * A required property is missing
	///  * An unknown property is present
	///  * A duplicate property is present
	///
	/// Note: Only ',' inside values can be escaped, other '\' are treated literally
	pub fn parse(&self, content: &str) -> Result<PropertyMap<'_>, String> {
		let mut properties = PropertyMap::new();
		let mut try_insert = |name: &String, value: &String| -> Result<(), String> {
			let trimmed_name = name.trim_end().trim_start();
			let trimmed_value = value.trim_end().trim_start();
			let prop = match self.properties.get(trimmed_name)
			{
				None => return Err(format!("Unknown property name: `{trimmed_name}` (with value: `{trimmed_value}`). Valid properties are:\n{}",
					self.properties.iter().fold(String::new(),
					|out, (name, prop)| out + format!(" - {name}: {prop}\n").as_str()))),
				Some(prop) => prop
			};

			if let Some((_, previous)) = properties
				.properties
				.insert(trimmed_name.to_string(), (prop, trimmed_value.to_string()))
			{
				return Err(format!("Duplicate property `{trimmed_name}`, previous value: `{previous}` current value: `{trimmed_value}`"));
			}

			Ok(())
		};

		let mut in_name = true;
		let mut name = String::new();
		let mut value = String::new();
		let mut escaped = 0usize;
		for c in content.chars() {
			if c == '\\' {
				escaped += 1;
			} else if c == '=' && in_name {
				in_name = false;
				(0..escaped).for_each(|_| name.push('\\'));
				escaped = 0;
			} else if c == ',' && !in_name {
				if escaped % 2 == 0
				// Not escaped
				{
					(0..escaped / 2).for_each(|_| value.push('\\'));
					escaped = 0;
					in_name = true;

					if let Err(e) = try_insert(&name, &value) {
						return Err(e);
					}
					name.clear();
					value.clear();
				} else {
					(0..(escaped - 1) / 2).for_each(|_| value.push('\\'));
					value.push(',');
					escaped = 0;
				}
			} else {
				if in_name {
					(0..escaped).for_each(|_| name.push('\\'));
					name.push(c)
				} else {
					(0..escaped).for_each(|_| value.push('\\'));
					value.push(c)
				}
				escaped = 0;
			}
		}
		(0..escaped).for_each(|_| value.push('\\'));
		if !in_name && value.trim_end().trim_start().is_empty() {
			return Err("Expected a value after last `=`".to_string());
		} else if name.is_empty() || value.is_empty() {
			return Err("Expected non empty property list.".to_string());
		}

		if let Err(e) = try_insert(&name, &value) {
			return Err(e);
		}

		if let Err(e) = self.properties.iter().try_for_each(|(key, prop)| {
			if !properties.properties.contains_key(key) {
				if let Some(default) = &prop.default {
					properties
						.properties
						.insert(key.clone(), (prop, default.clone()));
				} else if prop.required {
					return Err(format!("Missing required property: {prop}"));
				}
			}
			Ok(())
		}) {
			Err(e)
		} else {
			Ok(properties)
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::document::element::ContainerElement;
	use crate::document::langdocument::LangDocument;
	use crate::elements::comment::Comment;
	use crate::elements::style::Style;
	use crate::elements::text::Text;
	use crate::parser::source::SourceFile;
	use crate::parser::source::Token;
	use std::rc::Rc;

	#[test]
	fn process_text_tests() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			"".to_string(),
			None,
		));
		let doc = LangDocument::new(source.clone(), None);

		assert_eq!(process_text(&doc, "a\nb"), "a b");
		assert_eq!(process_text(&doc, "a\n\nb"), "a b"); // Should never happen but why not
		assert_eq!(process_text(&doc, "a\\b"), "ab");
		assert_eq!(process_text(&doc, "a\\\nb"), "a\nb");
		assert_eq!(process_text(&doc, "a\\\\b"), "a\\b");
		assert_eq!(process_text(&doc, "a\\\\\nb"), "a\\ b");
		assert_eq!(process_text(&doc, "\na"), "a");

		let tok = Token::new(0..0, source);
		doc.push(Box::new(Paragraph {
			location: tok.clone(),
			content: Vec::new(),
		}));

		// Comments are ignored (kind => Invisible)
		(&doc as &dyn Document)
			.last_element_mut::<Paragraph>()
			.unwrap()
			.push(Box::new(Comment::new(tok.clone(), "COMMENT".to_string())));
		assert_eq!(process_text(&doc, "\na"), "a");

		// A space is appended as previous element is inline
		(&doc as &dyn Document)
			.last_element_mut::<Paragraph>()
			.unwrap()
			.push(Box::new(Text::new(tok.clone(), "TEXT".to_string())));
		assert_eq!(process_text(&doc, "\na"), " a");

		(&doc as &dyn Document)
			.last_element_mut::<Paragraph>()
			.unwrap()
			.push(Box::new(Style::new(tok.clone(), 0, false)));
		assert_eq!(process_text(&doc, "\na"), " a");
	}

	#[test]
	fn process_escaped_tests() {
		assert_eq!(
			process_escaped(
				'\\',
				"%",
				"escaped: \\%, also escaped: \\\\\\%, untouched: \\a"
			),
			"escaped: %, also escaped: \\%, untouched: \\a"
		);
		assert_eq!(
			process_escaped('"', "><)))°>", "Escaped fish: \"><)))°>"),
			"Escaped fish: ><)))°>".to_string()
		);
		assert_eq!(
			process_escaped('\\', "]", "Escaped \\]"),
			"Escaped ]".to_string()
		);
		assert_eq!(
			process_escaped('\\', "]", "Unescaped \\\\]"),
			"Unescaped \\\\]".to_string()
		);
		assert_eq!(
			process_escaped('\\', "]", "Escaped \\\\\\]"),
			"Escaped \\]".to_string()
		);
		assert_eq!(
			process_escaped('\\', "]", "Unescaped \\\\\\\\]"),
			"Unescaped \\\\\\\\]".to_string()
		);
		assert_eq!(process_escaped('\\', ")", "A\\)B\\"), "A)B".to_string(),);
		assert_eq!(process_escaped('\\', ")", "A\\)B\\\\"), "A)B\\".to_string(),);
	}

	#[test]
	fn property_parser_tests() {
		let mut properties = HashMap::new();
		properties.insert(
			"width".to_string(),
			Property::new(true, "Width of the element in em".to_string(), None),
		);
		properties.insert(
			"length".to_string(),
			Property::new(false, "Length in cm".to_string(), None),
		);
		properties.insert(
			"angle".to_string(),
			Property::new(
				true,
				"Angle in degrees".to_string(),
				Some("180".to_string()),
			),
		);
		properties.insert(
			"weight".to_string(),
			Property::new(false, "Weight in %".to_string(), Some("0.42".to_string())),
		);

		let parser = PropertyParser { properties };
		let pm = parser.parse("width=15,length=-10").unwrap();

		// Ok
		assert_eq!(pm.get("width", |_, s| s.parse::<i32>()).unwrap().1, 15);
		assert_eq!(pm.get("length", |_, s| s.parse::<i32>()).unwrap().1, -10);
		assert_eq!(pm.get("angle", |_, s| s.parse::<f64>()).unwrap().1, 180f64);
		assert_eq!(pm.get("angle", |_, s| s.parse::<i32>()).unwrap().1, 180);
		assert_eq!(
			pm.get("weight", |_, s| s.parse::<f32>()).unwrap().1,
			0.42f32
		);
		assert_eq!(
			pm.get("weight", |_, s| s.parse::<f64>()).unwrap().1,
			0.42f64
		);

		// Error
		assert!(pm.get("length", |_, s| s.parse::<u32>()).is_err());
		assert!(pm.get("height", |_, s| s.parse::<f64>()).is_err());

		// Missing property
		assert!(parser.parse("length=15").is_err());

		// Defaults
		assert!(parser.parse("width=15").is_ok());
		assert_eq!(
			parser
				.parse("width=0,weight=0.15")
				.unwrap()
				.get("weight", |_, s| s.parse::<f32>())
				.unwrap()
				.1,
			0.15f32
		);
	}
}
