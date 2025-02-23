use serde::Serialize;

use crate::compiler::compiler::Compiler;
use crate::compiler::output::CompilerOutput;
use crate::document::element::ElemKind;
use crate::elements::text::elem::Text;
use crate::parser::scope::Scope;
use crate::parser::scope::ScopeAccessor;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use crate::parser::state::ParseMode;
use crate::parser::translation::TranslationAccessors;
use crate::parser::translation::TranslationUnit;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use super::element::ContainerElement;
use super::element::Element;

/// Internal name for variables
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VariableName(pub String);

impl core::fmt::Display for VariableName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

/// Holds the generated ast from a variable
#[derive(Debug)]
pub struct VariableExpansion
{
	location: Token,
	content: Vec<Rc<RefCell<Scope>>>,
}

impl Element for VariableExpansion {
    fn location(&self) -> &Token {
        &self.location()
    }

    fn kind(&self) -> super::element::ElemKind {
		ElemKind::Special
    }

    fn element_name(&self) -> &'static str {
        "Variable Expansion"
    }

    fn compile(
		    &self,
		    _scope: Rc<RefCell<Scope>>,
		    compiler: &Compiler,
		    output: &mut CompilerOutput,
	    ) -> Result<(), Vec<crate::parser::reports::Report>> {
		for (scope, elem) in self.content[0].content_iter()
		{
			elem.compile(scope, compiler, output)?;
		}
		Ok(())
    }
}

impl ContainerElement for VariableExpansion {
    fn contained(&self) -> &[Rc<RefCell<Scope>>] {
        self.content.as_slice()
    }
}

/// Trait for document variables
pub trait Variable {
	/// Gets the definition location of the variable
	fn location(&self) -> &Token;

	/// Gets the variable typename for serialization
	fn variable_typename(&self) -> &'static str;

	//fn serialize_inner(&self) -> ();

	/// Gets the name of the variable
	fn name(&self) -> &VariableName;

	/// Converts variable to a string
	fn to_string(&self) -> String;

	/// The token when the variable value was defined from
	fn value_token(&self) -> &Token;

	/// Expands the variable when it is requested
	fn expand<'u>(&self, unit: &mut TranslationUnit<'u>, location: Token);
}

impl core::fmt::Debug for dyn Variable {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{:#?}{{{}}}", self.name(), self.to_string())
	}
}

/// Base variables, a variable that is parsed when invoked
#[derive(Debug)]
pub struct BaseVariable {
	location: Token,
	name: VariableName,
	value_token: Token,
	value: String,
}

impl BaseVariable {
	pub fn new(location: Token, name: VariableName, value_token: Token, value: String) -> Self {
		Self {
			location,
			name,
			value_token,
			value,
		}
	}
}

impl Variable for BaseVariable {
	fn location(&self) -> &Token { &self.location }

	fn variable_typename(&self) -> &'static str {
	    "base_variable"
	}

	fn name(&self) -> &VariableName { &self.name }

	fn to_string(&self) -> String { self.value.clone() }

	fn value_token(&self) -> &Token { &self.value_token }

	fn expand<'u>(&self, unit: &mut TranslationUnit<'u>, location: Token) {
		// Create variable content
		let source = Arc::new(VirtualSource::new(
			self.location().clone(),
			format!(":VAR:{}", self.name()),
			self.to_string(),
		));

		// Expand & parse variable content
		let scope = unit.with_child(source, ParseMode::default(), false, |unit, scope| {
			unit.parser().parse(unit);

			scope
		});

		// Store expanded variable to a new element
		let expanded = Arc::new(VariableExpansion {
			location,
			content: vec![scope],
		});
		
		unit.add_content(expanded);
	}
}

/// A path-aware variable, expanded as text when processed
#[derive(Debug)]
pub struct PathVariable {
	location: Token,
	name: VariableName,
	value_token: Token,
	path: PathBuf,
}

impl PathVariable {
	pub fn new(location: Token, name: VariableName, value_token: Token, path: PathBuf) -> Self {
		Self {
			location,
			name,
			value_token,
			path,
		}
	}
}

impl Variable for PathVariable {
	fn location(&self) -> &Token { &self.location }

	fn variable_typename(&self) -> &'static str {
	    "path_variable"
	}

	fn name(&self) -> &VariableName { &self.name }

	fn to_string(&self) -> String { self.path.to_str().unwrap().to_string() }

	fn value_token(&self) -> &Token { &self.value_token }

	fn expand<'u>(&self, unit: &mut TranslationUnit<'u>, location: Token) {
		let source = Arc::new(VirtualSource::new(
			location.clone(),
			self.name().to_string(),
			self.to_string(),
		));

		// Expand variable as [`Text`]
		let scope = unit.with_child(source.clone(), ParseMode::default(), false, |_, scope| {
			scope.add_content(Arc::new(Text {
				location: (source as Arc<dyn Source>).into(),
				// FIXME this should depend of the current work dir
				content: self.to_string(),
			}));
			scope
		});

		// Add expanded variable
		unit.add_content(Arc::new(VariableExpansion {
			location,
			content: vec![scope],
		}));
	}
}
