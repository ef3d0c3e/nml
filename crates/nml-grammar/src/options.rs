use std::fmt;

use nml_source::Token;

/// The options a rule accepts in its `[key=value, …]` list.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OptionSchema {
	pub options: Vec<OptionSpec>,
}

impl OptionSchema {
	pub fn new(options: impl IntoIterator<Item = OptionSpec>) -> Self {
		Self {
			options: options.into_iter().collect(),
		}
	}

	pub fn get(&self, name: &str) -> Option<&OptionSpec> {
		self.options.iter().find(|option| option.name == name)
	}
}

/// One option of a schema.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionSpec {
	pub name: String,
	pub kind: OptionKind,
	/// The value when the option is not given. An option without a default is required.
	pub default: Option<OptionValue>,
	/// Documentation, for LSP hover and completion.
	pub doc: String,
}

impl OptionSpec {
	/// A required option.
	pub fn new(name: impl Into<String>, kind: OptionKind) -> Self {
		Self {
			name: name.into(),
			kind,
			default: None,
			doc: String::new(),
		}
	}

	pub fn with_default(mut self, default: OptionValue) -> Self {
		self.default = Some(default);
		self
	}

	pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
		self.doc = doc.into();
		self
	}
}

#[derive(Debug, Clone, PartialEq)]
pub enum OptionKind {
	Text,
	Integer,
	Number,
	/// `true` or `false`. The key alone, without a value, means `true`.
	Bool,
	/// One of these words.
	Choice(Vec<String>),
}

impl fmt::Display for OptionKind {
	/// The name used in diagnostics: `Integer`, `Number`, `Bool`, or the choices.
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			OptionKind::Text => write!(f, "Text"),
			OptionKind::Integer => write!(f, "Integer"),
			OptionKind::Number => write!(f, "Number"),
			OptionKind::Bool => write!(f, "Bool"),
			OptionKind::Choice(choices) => write!(f, "{}", choices.join(", ")),
		}
	}
}

#[derive(Debug, Clone, PartialEq)]
pub enum OptionValue {
	Text(String),
	Integer(i64),
	Number(f64),
	Bool(bool),
}

impl OptionKind {
	/// Converts the text of a value, or `None` for the key alone.
	pub(crate) fn convert(&self, text: Option<&str>) -> Option<OptionValue> {
		let Some(text) = text else {
			return (*self == OptionKind::Bool).then_some(OptionValue::Bool(true));
		};
		match self {
			OptionKind::Text => Some(OptionValue::Text(text.to_string())),
			OptionKind::Integer => text.parse().ok().map(OptionValue::Integer),
			OptionKind::Number => text
				.parse()
				.ok()
				.filter(|n: &f64| n.is_finite())
				.map(OptionValue::Number),
			OptionKind::Bool => match text {
				"true" => Some(OptionValue::Bool(true)),
				"false" => Some(OptionValue::Bool(false)),
				_ => None,
			},
			OptionKind::Choice(choices) => choices
				.iter()
				.any(|choice| choice == text)
				.then(|| OptionValue::Text(text.to_string())),
		}
	}
}

/// A parsed option list.
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
	/// The entries as written.
	pub entries: Vec<OptionEntry>,
	/// The value of every option of the schema that has one (given or default), in schema
	/// order.
	pub values: Vec<(String, OptionValue)>,
}

impl Options {
	pub fn get(&self, name: &str) -> Option<&OptionValue> {
		self.values
			.iter()
			.find(|(key, _)| key == name)
			.map(|(_, value)| value)
	}
}

/// One `key=value` entry as written.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionEntry {
	pub key: Token,
	/// The value as written (with quotes), if any.
	pub value_token: Option<Token>,
	/// The value without quotes and escapes.
	pub value: Option<String>,
}
