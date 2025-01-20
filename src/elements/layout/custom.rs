use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::CompilerOutput;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::parser::parser::ParserState;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use std::any::Any;
use std::collections::HashMap;
use std::ops::Range;
use std::str::FromStr;

use super::data::LayoutType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutToken {
	Begin,
	Next,
	End,
}

impl FromStr for LayoutToken {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"Begin" | "begin" => Ok(LayoutToken::Begin),
			"Next" | "next" => Ok(LayoutToken::Next),
			"End" | "end" => Ok(LayoutToken::End),
			_ => Err(format!("Unable to find LayoutToken with name: {s}")),
		}
	}
}

#[derive(Debug)]
#[auto_registry::auto_registry(registry = "layouts")]
pub struct Centered(PropertyParser);

impl Default for Centered {
	fn default() -> Self {
		let mut properties = HashMap::new();
		properties.insert(
			"style".to_string(),
			Property::new(
				"Additional style for the split".to_string(),
				Some("".to_string()),
			),
		);

		Self(PropertyParser { properties })
	}
}

impl LayoutType for Centered {
	fn name(&self) -> &'static str { "Centered" }

	fn expects(&self) -> Range<usize> { 1..1 }

	fn parse_properties(
		&self,
		reports: &mut Vec<Report>,
		state: &ParserState,
		token: Token,
	) -> Option<Box<dyn Any>> {
		let properties = match self.0.parse("Centered Layout", reports, state, token) {
			Some(props) => props,
			None => return None,
		};

		let style = match properties.get(reports, "style", |_, value| {
			Result::<_, String>::Ok(value.value.clone())
		}) {
			Some(style) => style,
			_ => return None,
		};

		Some(Box::new(style))
	}

	fn compile<'e>(
		&'e self,
		token: LayoutToken,
		_id: usize,
		properties: &'e Box<dyn Any>,
		compiler: &'e Compiler,
		_document: &'e dyn Document,
		mut output: CompilerOutput,
	) -> Result<CompilerOutput, Vec<Report>> {
		match compiler.target() {
			HTML => {
				let style = match properties.downcast_ref::<String>().unwrap().as_str() {
					"" => "".to_string(),
					str => format!(r#" style={}"#, Compiler::sanitize(compiler.target(), str)),
				};
				match token {
					LayoutToken::Begin => output.add_content(format!(r#"<div class="centered"{style}>"#)),
					LayoutToken::Next => panic!(),
					LayoutToken::End => output.add_content(r#"</div>"#.to_string()),
				}
			}
			_ => todo!(""),
		}
		Ok(output)
	}
}

#[derive(Debug)]
#[auto_registry::auto_registry(registry = "layouts")]
pub struct Split(PropertyParser);

impl Default for Split {
	fn default() -> Self {
		let mut properties = HashMap::new();
		properties.insert(
			"style".to_string(),
			Property::new(
				"Additional style for the split".to_string(),
				Some("".to_string()),
			),
		);

		Self(PropertyParser { properties })
	}
}

impl LayoutType for Split {
	fn name(&self) -> &'static str { "Split" }

	fn expects(&self) -> Range<usize> { 2..usize::MAX }

	fn parse_properties(
		&self,
		reports: &mut Vec<Report>,
		state: &ParserState,
		token: Token,
	) -> Option<Box<dyn Any>> {
		let properties = match self.0.parse("Split Layout", reports, state, token) {
			Some(props) => props,
			None => return None,
		};

		let style = match properties.get(reports, "style", |_, value| {
			Result::<_, String>::Ok(value.value.clone())
		}) {
			Some(style) => style,
			_ => return None,
		};

		Some(Box::new(style))
	}

	fn compile<'e>(
		&'e self,
		token: LayoutToken,
		_id: usize,
		properties: &'e Box<dyn Any>,
		compiler: &'e Compiler,
		_document: &'e dyn Document,
		mut output: CompilerOutput,
	) -> Result<CompilerOutput, Vec<Report>> {
		match compiler.target() {
			HTML => {
				let style = match properties.downcast_ref::<String>().unwrap().as_str() {
					"" => "".to_string(),
					str => format!(r#" style={}"#, Compiler::sanitize(compiler.target(), str)),
				};
				match token {
					LayoutToken::Begin => output.add_content(format!(
						r#"<div class="split-container"><div class="split"{style}>"#
					)),
					LayoutToken::Next => output.add_content(format!(r#"</div><div class="split"{style}>"#)),
					LayoutToken::End => output.add_content(r#"</div></div>"#.to_string()),
				}
			}
			_ => todo!(""),
		}
		Ok(output)
	}
}

#[derive(Debug)]
#[auto_registry::auto_registry(registry = "layouts")]
pub struct Spoiler(PropertyParser);

impl Default for Spoiler {
	fn default() -> Self {
		let mut properties = HashMap::new();
		properties.insert(
			"title".to_string(),
			Property::new("Spoiler title".to_string(), Some("".to_string())),
		);

		Self(PropertyParser { properties })
	}
}

impl LayoutType for Spoiler {
	fn name(&self) -> &'static str { "Spoiler" }

	fn expects(&self) -> Range<usize> { 1..1 }

	fn parse_properties(
		&self,
		reports: &mut Vec<Report>,
		state: &ParserState,
		token: Token,
	) -> Option<Box<dyn Any>> {
		let properties = match self.0.parse("Spoiler Layout", reports, state, token) {
			Some(props) => props,
			None => return None,
		};

		let title = match properties.get(reports, "title", |_, value| {
			Result::<_, String>::Ok(value.value.clone())
		}) {
			Some(title) => title,
			_ => return None,
		};

		Some(Box::new(title))
	}

	fn compile<'e>(
		&'e self,
		token: LayoutToken,
		_id: usize,
		properties: &'e Box<dyn Any>,
		compiler: &'e Compiler,
		_document: &'e dyn Document,
		mut output: CompilerOutput,
	) -> Result<CompilerOutput, Vec<Report>> {
		match compiler.target() {
			HTML => {
				let title = properties.downcast_ref::<String>().unwrap();
				match token {
					LayoutToken::Begin => output.add_content(format!(
						r#"<details class="spoiler"><summary>{}</summary>"#,
						Compiler::sanitize(compiler.target(), title)
					)),
					LayoutToken::End => output.add_content(r#"</details>"#.to_string()),
					_ => panic!(),
				}
			}
			_ => todo!(""),
		}
		Ok(output)
	}
}
