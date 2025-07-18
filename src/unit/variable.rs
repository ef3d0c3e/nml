use downcast_rs::impl_downcast;
use downcast_rs::Downcast;
use mlua::UserData;
use parking_lot::RwLock;
use serde::Deserialize;
use serde::Serialize;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::elements::text::elem::Text;
use crate::elements::variable::elem::VariableSubstitution;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use crate::parser::state::ParseMode;
use std::sync::Arc;

use super::element::ContainerElement;
use super::element::ElemKind;
use super::element::Element;
use super::element::LinkableElement;
use super::element::ReferenceableElement;
use super::scope::Scope;
use super::scope::ScopeAccessor;
use super::translation::TranslationUnit;

/// Holds the name of a variable (as a string)
///
/// Constructed using [`TryFrom<&str> for VariableName`]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VariableName(pub String);

impl core::fmt::Display for VariableName {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.0)
	}
}

impl TryFrom<&str> for VariableName {
	type Error = String;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		let mut it = value.chars();
		while let Some(c) = it.next() {
			if c.is_ascii_punctuation() && !(c == '.' || c == '_') {
				return Err(format!(
					"Variable name `{value}` cannot contain punctuation codepoint: `{c}`"
				));
			}
			if c.is_whitespace() {
				return Err(format!(
					"Variable name `{value}` cannot contain whitespaces: `{c}`"
				));
			}
			if c.is_control() {
				return Err(format!(
					"Variable name `{value}` cannot contain control codepoint: `{c}`"
				));
			}
		}
		Ok(VariableName(value.into()))
	}
}

/// Visibility attributes for variables
/// Variables tagged `Internal` may only be accessed from the scope and its children.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariableVisibility {
	/// Available from parent scope
	Exported,
	/// Internal to scope
	Internal,
}

impl std::fmt::Display for VariableVisibility {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			VariableVisibility::Exported => write!(f, "exported"),
			VariableVisibility::Internal => write!(f, "internal"),
		}
	}
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum VariableMutability {
	Mutable,
	Immutable,
}

impl std::fmt::Display for VariableMutability {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			VariableMutability::Mutable => write!(f, "mutable"),
			VariableMutability::Immutable => write!(f, "immutable"),
		}
	}
}

/// Trait for document variables
pub trait Variable: Downcast + core::fmt::Debug + Send + Sync {
	/// Gets the definition location of the variable
	fn location(&self) -> &Token;

	/// Gets the variable typename for serialization
	/// This name must remain unique to a variable
	fn variable_typename(&self) -> &'static str;

	/// Gets the name of the variable
	fn name(&self) -> &VariableName;

	/// Gets the visibility of the variable
	fn visibility(&self) -> &VariableVisibility;

	/// Gets the mutability of the variable
	fn mutability(&self) -> &VariableMutability;

	/// The token when the variable value was defined from
	fn value_token(&self) -> &Token;

	/// Expands the variable when it is requested
	fn expand<'u>(&self, unit: &mut TranslationUnit, location: Token) -> Arc<RwLock<Scope>>;

	fn to_string(&self) -> String;
}
impl_downcast!(Variable);

/// Variable that can be expanded to content
#[derive(Debug)]
pub struct ContentVariable {
	pub location: Token,
	pub name: VariableName,
	pub visibility: VariableVisibility,
	pub mutability: VariableMutability,
	pub content: Arc<dyn Source>,
}

impl Variable for ContentVariable {
	fn location(&self) -> &Token {
		&self.location
	}

	fn variable_typename(&self) -> &'static str {
		"content"
	}

	fn name(&self) -> &VariableName {
		&self.name
	}

	fn visibility(&self) -> &VariableVisibility {
		&self.visibility
	}

	fn mutability(&self) -> &VariableMutability {
		&self.mutability
	}

	fn value_token(&self) -> &Token {
		self.content.location().map_or(&self.location, |loc| &loc)
	}

	fn expand<'u>(&self, unit: &mut TranslationUnit, location: Token) -> Arc<RwLock<Scope>> {
		// Parse content
		let content = unit.with_child(
			self.content.clone(),
			ParseMode::default(),
			true,
			|unit, scope| {
				unit.parser.clone().parse(unit);
				scope
			},
		);
		content
	}

	fn to_string(&self) -> String {
		self.content.content().into()
	}
}

/// Values for property variables
#[derive(Debug)]
pub enum PropertyValue {
	Integer(i64),
	String(String),
}

impl ToString for PropertyValue {
	fn to_string(&self) -> String {
		match self {
			PropertyValue::Integer(i) => i.to_string(),
			PropertyValue::String(s) => s.clone(),
		}
	}
}

/// Variable representing a property
#[derive(Debug)]
pub struct PropertyVariable {
	// TODO: Mutability restrictions
	pub location: Token,
	pub name: VariableName,
	pub visibility: VariableVisibility,
	pub mutability: VariableMutability,
	pub value: PropertyValue,
	pub value_token: Token,
}

impl PropertyVariable {
	pub fn value(&self) -> &PropertyValue {
		&self.value
	}
}

impl Variable for PropertyVariable {
	fn location(&self) -> &Token {
		&self.location
	}

	fn variable_typename(&self) -> &'static str {
		"property"
	}

	fn name(&self) -> &VariableName {
		&self.name
	}

	fn visibility(&self) -> &VariableVisibility {
		&self.visibility
	}

	fn mutability(&self) -> &VariableMutability {
		&self.mutability
	}

	fn value_token(&self) -> &Token {
		&self.value_token
	}

	fn expand<'u>(&self, unit: &mut TranslationUnit, location: Token) -> Arc<RwLock<Scope>> {
		// Generate source for scope
		let definition_source = Arc::new(VirtualSource::new(
			self.location.clone(),
			format!(":VAR:Definition for `{}`", &self.name.0),
			self.value_token.content().into(),
		)) as Arc<dyn Source>;
		// Add content to scope
		let content = unit.with_child(
			definition_source.clone(),
			ParseMode::default(),
			true,
			|unit, scope| {
				scope.add_content(Arc::new(Text {
					location: definition_source.into(),
					content: self.value.to_string(),
				}));
				scope
			},
		);
		content
	}

	fn to_string(&self) -> String {
		self.value.to_string()
	}
}
