use std::collections::HashMap;
use std::fmt::Display;
use std::ops::Range;

use ariadne::Fmt;

use crate::elements::anchor::rule;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::util::escape_source;
use crate::unit::translation::TranslationUnit;

use super::reports::Report;
use super::source::Token;

/// Represents an individual property, with an optional default value
#[derive(Debug)]
pub struct Property {
	description: String,
	default: Option<String>,
}

impl Property {
	pub fn new(description: String, default: Option<String>) -> Self {
		Self {
			description,
			default,
		}
	}
}

impl core::fmt::Display for Property {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self.default.as_ref() {
			None => write!(f, "{}", self.description),
			Some(default) => write!(f, "{} (Default: {})", self.description, default),
		}
	}
}

/// The parsed value of a property
///
/// If property was set using default, the ranges correspond to the total range of the property string
/// The ranges are used to provide diagnostics as well as semantics via the language server.
#[derive(Debug)]
pub struct PropertyValue {
	pub name_range: Range<usize>,
	pub value_range: Range<usize>,
	pub value: String,
}

pub struct PropertyMap<'s> {
	pub token: Token,
	pub rule_name: &'s str,
	pub colors: ReportColors,
	pub properties: HashMap<String, (&'s Property, PropertyValue)>,
}

impl<'s> PropertyMap<'s> {
	pub fn new(token: Token, rule_name: &'s str, colors: ReportColors) -> Self {
		Self {
			token,
			rule_name,
			colors,
			properties: HashMap::new(),
		}
	}

	/// Get a value by key
	///
	/// # Returned value
	///
	///  * `Some(T)` on success
	///  * `None` if the key is not found or the parsing function `f` fails
	/// 	(Note) In this case, reports should have been set
	pub fn get<T, E, F>(&mut self, unit: &mut TranslationUnit, key: &str, f: F) -> Option<T>
	where
		F: FnOnce(&Property, PropertyValue) -> Result<T, E>,
		E: Display,
	{
		match self.properties.remove(key) {
			None => report_err!(
				unit,
				self.token.source(),
				format!("Failed to parse {} properties", self.rule_name),
				span(
					self.token.range.clone(),
					format!("Missing property {}", key.fg(self.colors.info),)
				),
			),
			Some((prop, val)) => {
				let range = val.value_range.clone();
				match f(prop, val) {
					Err(err) => report_err!(
						unit,
						self.token.source(),
						format!("Failed to parse {} properties", self.rule_name),
						span(
							range,
							format!(
								"Unable to parse property {}: {err}",
								key.fg(self.colors.info),
							)
						),
					),
					Ok(parsed) => return Some(parsed),
				}
			}
		}
		None
	}

	/// Get a value by key with a default
	///
	/// # Returned value
	///
	///  * `Some(T)` on success
	///  * `None` if the parsing function `f` fails
	/// 	(Note) In this case, reports should have been set
	pub fn get_or<T, E, F>(
		&mut self,
		unit: &mut TranslationUnit,
		key: &str,
		default: T,
		f: F,
	) -> Option<T>
	where
		F: FnOnce(&Property, PropertyValue) -> Result<T, E>,
		E: Display,
	{
		match self.properties.remove(key) {
			None => return Some(default),
			Some((prop, val)) => {
				let range = val.value_range.clone();
				match f(prop, val) {
					Err(err) => report_err!(
						unit,
						self.token.source(),
						format!("Failed to parse {} properties", self.rule_name),
						span(
							range,
							format!(
								"Unable to parse property {}: {err}",
								key.fg(self.colors.info),
							)
						),
					),
					Ok(parsed) => return Some(parsed),
				}
			}
		}
		None
	}

	/// Get an optional value by key
	///
	/// # Returned value
	///
	///  * `Some(Option<T>)` on success
	///  * `None` if the parsing function `f` fails
	/// 	(Note) In this case, reports should have been set
	pub fn get_opt<T, E, F>(
		&mut self,
		unit: &mut TranslationUnit,
		key: &str,
		f: F,
	) -> Option<Option<T>>
	where
		F: FnOnce(&Property, PropertyValue) -> Result<T, E>,
		E: Display,
	{
		match self.properties.remove(key) {
			None => return Some(None),
			Some((prop, val)) => {
				let range = val.value_range.clone();
				match f(prop, val) {
					Err(err) => report_err!(
						unit,
						self.token.source(),
						format!("Failed to parse {} properties", self.rule_name),
						span(
							range,
							format!(
								"Unable to parse property {}: {err}",
								key.fg(self.colors.info),
							)
						),
					),
					Ok(parsed) => return Some(Some(parsed)),
				}
			}
		}
		None
	}
}

/// Parser for properties
#[derive(Debug)]
pub struct PropertyParser {
	pub properties: HashMap<String, Property>,
}

impl PropertyParser {
	/// Gets the list of all properties as a string for displaying
	fn allowed_properties(&self, colors: &ReportColors) -> String {
		self.properties
			.iter()
			.fold(String::new(), |out, (name, prop)| {
				out + format!("\n - {} : {}", name.fg(colors.info), prop.description).as_str()
			})
	}

	/// Parses properties string "prop1=value1, prop2 = val\,2" -> {prop1: value1, prop2: val,2}
	///
	/// # Key-value pair
	///
	/// Property names/values are separated by a single '=' that cannot be escaped.
	/// Therefore names cannot contain the '=' character.
	///
	/// # Language Server
	///
	/// This function also processes properties to add them to the language server's semantics.
	/// It uses the [`Semantics::add_to_queue()`] so it is safe to add other semantics after this function call.
	///
	/// # Example
	///
	/// ```
	/// let mut properties = HashMap::new();
	/// properties.insert("width".to_string(),
	/// 	Property::new("Width of the element in em".to_string(), None));
	///
	/// let parser = PropertyParser { properties };
	/// let source = VirtualSource::new(.. "width=15" ..)
	/// let properties = match parser.parse("Element", &mut reports, &state, source).unwrap()
	/// {
	/// 	Some(properties) => properties,
	/// 	None => return reports,
	/// };
	///
	/// assert_eq!(properties.get(&mut reports, "width", |_, value| value.parse::<i32>()).unwrap(), 15);
	/// ```
	/// # Return value
	///
	/// `Some(properties)` is returned on success. On failure, `None` is returned and the reports will have been populated.
	///
	/// Note: Only ',' inside values can be escaped, other '\' are treated literally
	pub fn parse<'s, 'u>(
		&'s self,
		rule_name: &'s str,
		unit: &mut TranslationUnit,
		token: Token,
	) -> Option<PropertyMap<'s>> {
		let mut pm = PropertyMap::new(token.clone(), rule_name, unit.colors().to_owned());
		let mut try_insert = |name: &String,
		                      name_range: Range<usize>,
		                      value: &String,
		                      value_range: Range<usize>|
		 -> bool {
			let trimmed_name = name.trim_start().trim_end();
			let trimmed_value = value.trim_start().trim_end();
			let prop = match self.properties.get(trimmed_name) {
				None => {
					report_err!(
						unit,
						token.source(),
						format!("Failed to parse {rule_name} properties"),
						span(
							name_range,
							format!(
								"Unknown property {}, allowed properties:{}",
								name.fg(unit.colors().info),
								self.allowed_properties(unit.colors())
							)
						),
					);
					return false;
				}
				Some(prop) => prop,
			};

			if let Some((_, previous)) = pm.properties.insert(
				trimmed_name.to_string(),
				(
					prop,
					PropertyValue {
						name_range: name_range.clone(),
						value_range: value_range.clone(),
						value: trimmed_value.to_string(),
					},
				),
			) {
				report_err!(
					unit,
					token.source(),
					format!("Failed to parse {rule_name} properties"),
					span(
						name_range.start..value_range.end,
						format!(
							"Duplicate property {}, current value: {}",
							name.fg(unit.colors().info),
							trimmed_value.fg(unit.colors().info),
						)
					),
					span(
						previous.value_range.clone(),
						format!("Previous value: {}", previous.value.fg(unit.colors().info),)
					)
				);
			}

			true
		};

		if !token.range.is_empty() {
			let mut in_name = true;
			let mut name = String::new();
			let mut name_range = token.start()..token.start();
			let mut value = String::new();
			let mut value_range = token.start()..token.start();
			let mut escaped = 0usize;
			for (pos, c) in token.source().content()[token.range.clone()].char_indices() {
				if c == '\\' {
					escaped += 1;
				} else if c == '=' && in_name {
					name_range.end = pos;
					value_range.start = pos + 1;
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
						value_range.end = pos;

						if !try_insert(&name, name_range.clone(), &value, value_range.clone()) {
							return None;
						}
						name_range.start = pos + 1;

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
						name.push(c);
					} else {
						(0..escaped).for_each(|_| value.push('\\'));
						value.push(c);
					}
					escaped = 0;
				}
			}
			(0..escaped).for_each(|_| value.push('\\'));
			if !in_name && value.trim_end().trim_start().is_empty() {
				report_err!(
					unit,
					token.source(),
					format!("Failed to parse {rule_name} properties"),
					span(
						value_range.start..token.end(),
						format!("Expected value after last '='",)
					),
				);
				return None;
			} else if name.is_empty() || value.is_empty() {
				report_err!(
					unit,
					token.source(),
					format!("Failed to parse {rule_name} properties"),
					span(
						name_range.start..token.end(),
						format!("Expected name/value pair separated by ','",)
					),
				);
				return None;
			}

			value_range.end = token.end();
			if !try_insert(&name, name_range.clone(), &value, value_range.clone()) {
				return None;
			}
		}

		unit.with_lsp(|lsp| {
			lsp.with_semantics(token.source(), |sems, tokens| {
				for (_, value) in pm.properties.values() {
					if value.name_range.start != 0 {
						sems.add_to_queue(
							value.name_range.start - 1..value.name_range.start,
							tokens.prop_comma,
						);
					}
					sems.add_to_queue(value.name_range.clone(), tokens.prop_name);
					sems.add_to_queue(
						value.name_range.end..value.value_range.start,
						tokens.prop_equal,
					);
					sems.add_to_queue(value.value_range.clone(), tokens.prop_value);
				}
			})
		});

		// Insert missing properties with a default
		for (name, prop) in &self.properties {
			if pm.properties.contains_key(name) {
				continue;
			}
			if let Some(default) = &prop.default {
				pm.properties.insert(
					name.to_owned(),
					(
						prop,
						PropertyValue {
							name_range: token.range.clone(),
							value_range: token.range.clone(),
							value: default.to_owned(),
						},
					),
				);
			}
		}

		Some(pm)
	}

	/// Parses properties from a token
	///
	/// This function will perform escaping on the token to parse the properties
	pub fn parse_token<'s, 'u>(
		&'s self,
		rule_name: &'s str,
		unit: &mut TranslationUnit,
		token: Token,
		escape: char,
		closing: &'static str,
	) -> Option<PropertyMap<'s>> {
		let prop_source = escape_source(
			token.source(),
			token.range,
			PathBuf::from(format!("{rule_name} Properties")),
			escape, closing
		);
		self.parse(
			rule_name,
			unit,
			Token::new(0..prop_source.content().len(), prop_source),
		)
	}
}

/*
#[cfg(test)]
mod tests {
	use parser::source::Source;
	use parser::source::SourceFile;
	use std::sync::Arc;

	use super::*;

	#[test]
	fn property_parser_tests() {
		let mut properties = HashMap::new();
		properties.insert(
			"width".to_string(),
			Property::new("Width of the element in em".to_string(), None),
		);
		properties.insert(
			"length".to_string(),
			Property::new("Length in cm".to_string(), None),
		);
		properties.insert(
			"angle".to_string(),
			Property::new("Angle in degrees".to_string(), Some("180".to_string())),
		);
		properties.insert(
			"weight".to_string(),
			Property::new("Weight in %".to_string(), Some("0.42".to_string())),
		);

		let langparser = LangParser::default();
		let state = ParserState::new(&langparser, None);
		let mut reports = vec![];

		let parser = PropertyParser { properties };
		let source = Arc::new(SourceFile::with_content(
			"".into(),
			"width=15,length=-10".into(),
			None,
		)) as Arc<dyn Source>;
		let pm = parser
			.parse("Test", &mut reports, &state, source.into())
			.unwrap();

		// Ok
		assert_eq!(
			pm.get(&mut reports, "width", |_, s| s.value.parse::<i32>())
				.unwrap(),
			15
		);
		assert_eq!(
			pm.get(&mut reports, "length", |_, s| s.value.parse::<i32>())
				.unwrap(),
			-10
		);
		assert_eq!(
			pm.get(&mut reports, "angle", |_, s| s.value.parse::<f64>())
				.unwrap(),
			180f64
		);
		assert_eq!(
			pm.get(&mut reports, "angle", |_, s| s.value.parse::<i32>())
				.unwrap(),
			180
		);
		assert_eq!(
			pm.get(&mut reports, "weight", |_, s| s.value.parse::<f32>())
				.unwrap(),
			0.42f32
		);
		assert_eq!(
			pm.get(&mut reports, "weight", |_, s| s.value.parse::<f64>())
				.unwrap(),
			0.42f64
		);

		// Error
		assert!(pm
			.get(&mut reports, "length", |_, s| s.value.parse::<u32>())
			.is_none());
		assert!(pm
			.get(&mut reports, "height", |_, s| s.value.parse::<f64>())
			.is_none());
	}
}
*/
