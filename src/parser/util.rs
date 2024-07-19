use std::collections::HashMap;

use unicode_segmentation::UnicodeSegmentation;

use crate::{document::{document::Document, element::ElemKind}, elements::paragraph::Paragraph};

/// Processes text for escape characters and paragraphing
pub fn process_text(document: &Document, content: &str) -> String
{
	let mut escaped = false;
	let mut newlines = 0usize; // Consecutive newlines
	//println!("Processing: [{content}]");
	let processed = content
		.grapheme_indices(true)
		.fold((String::new(), None),
		|(mut out, prev), (_pos, g)| {
		if newlines != 0 && g != "\n"
		{
			newlines = 0;

			// Add a whitespace if necessary
			match out.chars().last()
			{
				Some(c) => {
					// NOTE: \n is considered whitespace, so previous codepoint can be \n
					// (Which can only be done by escaping it)
					if !c.is_whitespace() || c == '\n'
					{
						out += " ";
					}
				}
				None => {
					if document.last_element::<Paragraph>(false)
						.and_then(|par| par.find_back(|e| e.kind() != ElemKind::Invisible)
						.and_then(|e| Some(e.kind() == ElemKind::Inline)))
							.unwrap_or(false)
					{
						out += " ";
					}
				} // Don't output anything
			}
		}

		// Output grapheme literally when escaped
		if escaped
		{
			escaped = false;
			return (out + g, Some(g));
		}
		// Increment newlines counter
		else if g == "\n"
		{
			newlines += 1;
			return (out, Some(g));
		}
		// Determine if escaped
		else if g == "\\"
		{
			escaped = !escaped;
			return (out, Some(g));
		}
		// Whitespaces
		else if g.chars().count() == 1 && g.chars().last().unwrap().is_whitespace()
		{
			// Content begins with whitespace
			if prev.is_none()
			{
				if document.last_element::<Paragraph>(false).is_some()
				{
					return (out+g, Some(g));
				}
				else
				{
					return (out, Some(g));
				}
			}
			// Consecutive whitespaces are converted to a single whitespace
			else if prev.unwrap().chars().count() == 1 &&
					prev.unwrap().chars().last().unwrap().is_whitespace()
			{
				return (out, Some(g));
			}
		}

		return (out + g, Some(g));
	}).0.to_string();

	return processed;
}

/// Processed a string and escapes a single token out of it
/// Escaped characters other than the [`token`] will be not be treated as escaped
///
/// # Example
/// ```
/// assert_eq!(process_escaped('\\', "%", "escaped: \\%, also escaped: \\\\\\%, untouched: \\a"),
/// "escaped: %, also escaped: \\%, untouched \\a");
/// ```
pub fn process_escaped<S: AsRef<str>>(escape: char, token: &'static str, content: S) -> String
{
	let mut processed = String::new();
	let mut escaped = 0;
	let mut token_it = token.chars().peekable();
	for c in content.as_ref().chars()
		.as_str()
			.trim_start()
			.trim_end()
			.chars()
			{
				if c == escape
				{
					escaped += 1;
				}
				else if escaped % 2 == 1 && token_it.peek().map_or(false, |p| *p == c)
				{
					let _ = token_it.next();
					if token_it.peek() == None
					{
						(0..((escaped-1)/2))
							.for_each(|_| processed.push(escape));
						escaped = 0;
						token_it = token.chars().peekable();
						processed.push_str(token);
					}
				}
				else
				{
					if escaped != 0
					{
						// Add untouched escapes
						(0..escaped).for_each(|_| processed.push('\\'));
						token_it = token.chars().peekable();
						escaped = 0;
					}
					processed.push(c);
				}
			}
	// Add trailing escapes
	(0..escaped).for_each(|_| processed.push('\\'));

	processed
}

#[derive(Debug)]
pub struct Property
{
	required: bool,
	description: String,
	default: Option<String>,
}

impl Property {
    pub fn new(required: bool, description: String, default: Option<String>) -> Self {
        Self { required, description, default }
    }
}

impl core::fmt::Display for Property
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self.default.as_ref()
		{
			None => write!(f, "{} {}", 
				["[Opt]", "[Req]"][self.required as usize],
				self.description),
			Some(default) => write!(f, "{} {} (Deafult: {})",
				["[Opt]", "[Req]"][self.required as usize],
				self.description,
				default)
		}
    }
}

#[derive(Debug)]
pub struct PropertyMap<'a>
{
	pub(crate) properties: HashMap<String, (&'a Property, String)>
}

impl<'a> PropertyMap<'a> {
    pub fn new() -> Self {
        Self { properties: HashMap::new() }
    }

	pub fn get<T, Error, F: FnOnce(&'a Property, &String) -> Result<T, Error>>(&self, name: &str, f: F)
		-> Result<(&'a Property, T), Error> {
		let (prop, value) = self.properties.get(name).unwrap();

		f(prop, value).and_then(|value| Ok((*prop, value)))
	}
}

pub struct PropertyParser {
	properties: HashMap<String, Property>,
}

impl PropertyParser {
    pub fn new(properties: HashMap<String, Property>) -> Self {
        Self { properties }
    }

	/// Attempts to build a default propertymap
	///
	/// Returns an error if at least one [`Property`] is required and doesn't provide a default
	pub fn default(&self) -> Result<PropertyMap<'_>, String> {
		let mut properties = PropertyMap::new();

		for (name, prop) in &self.properties
		{
			match (prop.required, prop.default.as_ref())
			{
    			(true, None) => return Err(format!("Missing property `{name}` {prop}")),
				(false, None) => {},
    			(_, Some(default)) => {
						properties.properties.insert(
						name.clone(),
						(prop, default.clone())
					);
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
	/// let properties = HashMap::new();
	/// properties.insert("width", Property::new(true, "Width of the element in em", None));
	///
	/// let parser = PropertyParser::new(properties);
	/// let pm = parser.parse("width=15").unwrap();
	///
	/// assert!(pm.get("width", |_, val| val.parse::<i32>()) == Ok(15));
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
		let mut try_insert = |name: &String, value: &String|
			-> Result<(), String> {
			let trimmed_name = name.trim_end().trim_start();
			let trimmed_value = value.trim_end().trim_start();
			let prop = match self.properties.get(trimmed_name)
			{
				None => return Err(format!("Unknown property name: `{trimmed_name}` (with value: `{trimmed_value}`). Valid properties are:\n{}",
					self.properties.iter().fold(String::new(),
					|out, (name, prop)| out + format!(" - {name}: {prop}\n").as_str()))),
				Some(prop) => prop
			};

			if let Some((_, previous)) = properties.properties.insert(
				trimmed_name.to_string(),
				(prop, trimmed_value.to_string()))
			{
				return Err(format!("Duplicate property `{trimmed_name}`, previous value: `{previous}` current value: `{trimmed_value}`"))
			}

			Ok(())
		};

		let mut in_name = true;
		let mut name = String::new();
		let mut value = String::new();
		let mut escaped = 0usize;
		for c in content.chars()
		{
			if c == '\\'
			{
				escaped += 1;
			}
			else if c == '=' && in_name
			{
				in_name = false;
				(0..escaped).for_each(|_| name.push('\\'));
				escaped = 0;
			}
			else if c == ',' && !in_name
			{
				if escaped % 2 == 0 // Not escaped
				{
					(0..escaped/2).for_each(|_| value.push('\\'));
					escaped = 0;
					in_name = true;

					if let Err(e) = try_insert(&name, &value) {
						return Err(e)
					}
					name.clear();
					value.clear();
				}
				else
				{
					(0..(escaped-1)/2).for_each(|_| value.push('\\'));
					value.push(',');
					escaped = 0;
				}
			}
			else
			{
				if in_name {
					(0..escaped).for_each(|_| name.push('\\'));
					name.push(c)
				}
				else {
					(0..escaped).for_each(|_| value.push('\\'));
					value.push(c)
				}
				escaped = 0;
			}
		}
		if !in_name && value.trim_end().trim_start().is_empty()
		{
			return Err("Expected a value after last `=`".to_string())
		}
		else if name.is_empty() || value.is_empty()
		{
			return Err("Expected non empty property list.".to_string());
		}

		if let Err(e) = try_insert(&name, &value) {
			return Err(e)
		}

		// TODO: Missing properties

		Ok(properties)
	}
}
