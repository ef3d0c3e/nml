use std::collections::HashMap;
use std::fmt::Display;
use std::ops::Range;

use ariadne::Fmt;
use lsp::semantic::Semantics;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;

use super::parser::ParserState;
use super::reports::Report;
use super::source::Token;

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

pub struct PropertyMap<'a> {
	pub token: Token,
	pub rule_name: &'a str,
	pub state: &'a ParserState<'a, 'a>,
	pub properties: HashMap<String, (&'a Property, PropertyValue)>,
}

impl<'a> PropertyMap<'a> {
	pub fn new(token: Token, rule_name: &'a str, state: &'a ParserState<'a, 'a>) -> Self {
		Self {
			token,
			rule_name,
			state,
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
	pub fn get<T, E, F>(&self, mut reports: &mut Vec<Report>, key: &str, f: F) -> Option<T>
	where
		F: FnOnce(&Property, &PropertyValue) -> Result<T, E>,
		E: Display,
	{
		match self.properties.get(key) {
			None => report_err!(
				&mut reports,
				self.token.source(),
				format!("Failed to parse {} properties", self.rule_name),
				span(
					self.token.range.clone(),
					format!(
						"Missing property {}",
						key.fg(self.state.parser.colors().info),
					)
				),
			),
			Some((prop, val)) => match f(prop, val) {
				Err(err) => report_err!(
					&mut reports,
					self.token.source(),
					format!("Failed to parse {} properties", self.rule_name),
					span(
						val.value_range.clone(),
						format!(
							"Unable to parse property {}: {err}",
							key.fg(self.state.parser.colors().info),
						)
					),
				),
				Ok(parsed) => return Some(parsed),
			},
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
		&self,
		mut reports: &mut Vec<Report>,
		key: &str,
		default: T,
		f: F,
	) -> Option<T>
	where
		F: FnOnce(&Property, &PropertyValue) -> Result<T, E>,
		E: Display,
	{
		match self.properties.get(key) {
			None => return Some(default),
			Some((prop, val)) => match f(prop, val) {
				Err(err) => report_err!(
					&mut reports,
					self.token.source(),
					format!("Failed to parse {} properties", self.rule_name),
					span(
						val.value_range.clone(),
						format!(
							"Unable to parse property {}: {err}",
							key.fg(self.state.parser.colors().info),
						)
					),
				),
				Ok(parsed) => return Some(parsed),
			},
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
		&self,
		mut reports: &mut Vec<Report>,
		key: &str,
		f: F,
	) -> Option<Option<T>>
	where
		F: FnOnce(&Property, &PropertyValue) -> Result<T, E>,
		E: Display,
	{
		match self.properties.get(key) {
			None => return Some(None),
			Some((prop, val)) => match f(prop, val) {
				Err(err) => report_err!(
					&mut reports,
					self.token.source(),
					format!("Failed to parse {} properties", self.rule_name),
					span(
						val.value_range.clone(),
						format!(
							"Unable to parse property {}: {err}",
							key.fg(self.state.parser.colors().info),
						)
					),
				),
				Ok(parsed) => return Some(Some(parsed)),
			},
		}
		None
	}
}

#[derive(Debug)]
pub struct PropertyParser {
	pub properties: HashMap<String, Property>,
}

impl PropertyParser {
	fn allowed_properties(&self, state: &ParserState) -> String {
		self.properties
			.iter()
			.fold(String::new(), |out, (name, prop)| {
				out + format!(
					"\n - {} : {}",
					name.fg(state.parser.colors().info),
					prop.description
				)
				.as_str()
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
	pub fn parse<'a>(
		&'a self,
		rule_name: &'a str,
		mut reports: &mut Vec<Report>,
		state: &'a ParserState<'a, 'a>,
		token: Token,
	) -> Option<PropertyMap<'a>> {
		let mut pm = PropertyMap::new(token.clone(), rule_name, state);
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
						&mut reports,
						token.source(),
						format!("Failed to parse {rule_name} properties"),
						span(
							name_range,
							format!(
								"Unknown property {}, allowed properties:{}",
								name.fg(state.parser.colors().info),
								self.allowed_properties(state)
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
					&mut reports,
					token.source(),
					format!("Failed to parse {rule_name} properties"),
					span(
						name_range.start..value_range.end,
						format!(
							"Duplicate property {}, current value: {}",
							name.fg(state.parser.colors().info),
							trimmed_value.fg(state.parser.colors().info),
						)
					),
					span(
						previous.value_range.clone(),
						format!(
							"Previous value: {}",
							previous.value.fg(state.parser.colors().info),
						)
					)
				);
			}

			true
		};

		if token.range.len() != 0 {
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
					&mut reports,
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
					&mut reports,
					token.source(),
					format!("Failed to parse {rule_name} properties"),
					span(
						name_range.start..token.end(),
						format!("Expected name/value pair after last ','",)
					),
				);
				return None;
			}

			value_range.end = token.end();
			if !try_insert(&name, name_range.clone(), &value, value_range.clone()) {
				return None;
			}
		}

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			for (_, (_, value)) in &pm.properties {
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
			//sems.add(matches.get(0).unwrap().start()..matches.get(0).unwrap().start()+1, tokens.media_sep);
			// Refname
			//sems.add(matches.get(0).unwrap().start()+1..matches.get(0).unwrap().start()+2, tokens.media_refname_sep);
			//sems.add(matches.get(1).unwrap().range(), tokens.media_refname);
			//sems.add(matches.get(1).unwrap().end()..matches.get(1).unwrap().end()+1, tokens.media_refname_sep);
		}

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
}

#[cfg(test)]
mod tests {
	use std::rc::Rc;

	use parser::langparser::LangParser;
	use parser::source::Source;
	use parser::source::SourceFile;

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
		let source = Rc::new(SourceFile::with_content(
			"".into(),
			"width=15,length=-10".into(),
			None,
		)) as Rc<dyn Source>;
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
