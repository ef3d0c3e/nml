use crate::document::variable::{ContentVariable, PropertyValue, PropertyVariable, Variable, VariableVisibility};
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::{RegexRule, Rule};
use crate::parser::scope::ScopeAccessor;
use crate::parser::source::{Cursor, Token};
use crate::parser::util::{escape_source, escape_text};
use ariadne::Fmt;
use document::variable::VariableName;
use parser::state::ParseMode;
use parser::translation::TranslationUnit;
use regex::{Captures, Regex};
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;


fn parse_delimited(content: &str, delim: &str) -> Option<usize>
{
	let mut escaped = 0usize;
	let mut it = content.char_indices();
	let mut end_pos = 0;

	loop {
		let Some((pos, c)) = it.next() else { return None };
		end_pos = pos;
		if c == '\\'
		{
			escaped += 1;
		} else if escaped % 2 == 1
		{
			
		}
		else if content[pos..].starts_with(delim)
		{
			break;
		}
		else
		{
			escaped = 0;
		}
	}
	Some(end_pos)
}

#[auto_registry::auto_registry(registry = "rules")]
pub struct VariableRule {
	/// Variable declaration regex
	decl_re: Regex,
	int_re: Regex,
}

impl Default for VariableRule
{
	fn default() -> Self
	{
		Self {
			decl_re: Regex::new(r#"(?:\n|^):(export|set)\s+(\w*)\s*(=?)\s*"#).unwrap(),
			int_re: Regex::new(r#"\s*(.*?)(true|false|(?:\+|-)?[0-9]*)[^\S\r\n]*(?:$|\n)"#).unwrap(),
		}
	}
}

impl Rule for VariableRule
{
    fn name(&self) -> &'static str {
        "Variable"
    }

    fn previous(&self) -> Option<&'static str> {
        Some("Link")
    }

    fn next_match(
		    &self,
		    _mode: &ParseMode,
		    cursor: &Cursor,
	    ) -> Option<(usize, Box<dyn Any>)> {
		self.decl_re
			.find_at(cursor.source().content(), cursor.pos())
			.map(|m| (m.start(), Box::new([false; 0]) as Box<dyn Any>))
    }

    fn on_match<'u>(
		    &self,
		    unit: &mut TranslationUnit<'u>,
		    cursor: &Cursor,
		    match_data: Box<dyn Any>,
	    ) -> Cursor {
		let source = cursor.source();
		let content = source.content();
		let captures = self.decl_re.captures_at(content, cursor.pos()).unwrap();
		assert_eq!(captures.get(0).unwrap().start(), cursor.pos());

		let mut end_pos = captures.get(0).unwrap().end();

		// `:expand <name>` 
		let keyword = captures.get(1).unwrap();
		let visibility = match keyword.as_str()
		{
			"set" => VariableVisibility::Internal,
			"export" => VariableVisibility::Exported,
			_ => panic!(),
		};
		let varname = captures.get(2).unwrap();
		if varname.as_str().is_empty()
		{
			report_err!(
				unit,
				cursor.source(),
				"Invalid variable name".into(),
				span(
					varname.range(),
					format!("Name is empty")
				)
			);
			return cursor.at(end_pos)
		}
		let name = match VariableName::try_from(varname.as_str())
		{
			Ok(name) => name,
			Err(err) => {
				report_err!(
					unit,
					cursor.source(),
					"Invalid variable name".into(),
					span(
						varname.range(),
						err
					)
				);
				return cursor.at(end_pos)
			}
		};

		let equal = captures.get(3).unwrap();
		if equal.as_str().is_empty()
		{
			report_err!(
				unit,
				cursor.source(),
				"Invalid variable definition".into(),
				span(
					equal.range(),
					format!("Missing '{}' symbol", "=".fg(unit.colors().info))
				)
			);
			return cursor.at(end_pos)
		}

		let delim = if content[end_pos..].starts_with("'''") { "'''" }
		else if content[end_pos..].starts_with("\"\"\"") { "\"\"\"" }
		else if content[end_pos..].starts_with("{{") { "}}" }
		else if content[end_pos..].starts_with("'") { "'" }
		else if content[end_pos..].starts_with("\"") { "\"" }
		else {
			// Parse as int
			let val_captures = self.int_re.captures_at(content, end_pos).unwrap();
			if !val_captures.get(1).unwrap().as_str().is_empty()
			{
				report_err!(
					unit,
					cursor.source(),
					"Invalid variable definition".into(),
					span(
						keyword.start()-1..end_pos,
						format!("Expected value after declaration")
					)
				);
				return cursor.at(end_pos);
			}
			let value = val_captures.get(2).unwrap();
			let val = if value.as_str() == "true"
			{
				1i64
			} else if value.as_str() == "false"
			{
				0i64
			} else {
				match value.as_str().parse::<i64>()
				{
					Ok(x) => x,
					Err(err) => {
						report_err!(
							unit,
							cursor.source(),
							"Invalid variable definition".into(),
							span(
								value.range(),
								format!("Failed to parse as integer: {err}")
							)
						);
						return cursor.at(end_pos);
					},
				}
			};

			Rc::new(PropertyVariable {
				location: Token::new(captures.get(0).unwrap().start()..val_captures.get(0).unwrap().end() - 1, cursor.source()),
				name,
				visibility,
				value: PropertyValue::Integer(val),
				value_token: Token::new(value.range(), cursor.source()),
			});
			return cursor.at(val_captures.get(0).unwrap().end() - 1);
		};

		let Some(value_len) = parse_delimited(&content[end_pos + delim.len()..], delim) else {
			report_err!(
				unit,
				cursor.source(),
				"Invalid variable definition".into(),
				span(
					keyword.start()-1..end_pos+delim.len(),
					format!("Missing end delimiter")
				),
				span(
					end_pos..end_pos+delim.len(),
					format!("Start delimiter here")
				)
			);
			return cursor.at(end_pos);
		};
		let content_range = end_pos + delim.len()..end_pos + delim.len() + value_len;
		// Insert as new source that can be parsed later
		if delim == "}}"
		{
			let content_source = escape_source(
				cursor.source(),
				content_range.clone(),
				format!(":VAR:Variable Content for `{}`", &name.0),
				'\\',
				delim
				);
			unit.get_scope()
				.insert_variable(name.clone(), Rc::new(ContentVariable {
					location: Token::new(keyword.start()-1..content_range.end, cursor.source()),
					name,
					visibility,
					content: content_source,
				}) as Rc<dyn Variable>);
		}
		// Insert as string property
		else
		{
			let value = escape_text('\\', delim, content[content_range.clone()].to_string(), false);
			unit.get_scope()
				.insert_variable(name.clone(), Rc::new(PropertyVariable {
				location: Token::new(keyword.start()-1..content_range.end, cursor.source()),
				name,
				visibility,
				value: PropertyValue::String(value),
				value_token: Token::new(content_range, cursor.source()),
			}) as Rc<dyn Variable>);
		}
		return cursor.at(end_pos + value_len + 2 * delim.len())
    }
}

#[auto_registry::auto_registry(registry = "rules")]
pub struct VariableSubstitutionRule {
	re: [Regex; 1],
}

impl Default for VariableSubstitutionRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r"%([^\s%]*)(%?)").unwrap()],
		}
	}
}

impl RegexRule for VariableSubstitutionRule {
	fn name(&self) -> &'static str { "Variable Substitution" }

	fn previous(&self) -> Option<&'static str> { Some("Variable") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn enabled(&self, _mode: &ParseMode, _id: usize) -> bool { true }

	fn on_regex_match<'u>(
		&self,
		_index: usize,
		unit: &'u mut TranslationUnit,
		token: Token,
		captures: Captures,
	) {
		let variable_name = captures.get(1).unwrap();
		let closing_token = captures.get(2).unwrap();
		if closing_token.is_empty()
		{
			report_err!(
				unit,
				token.source(),
				"Unterminated variable substitution".into(),
				span(
					variable_name.end()-1..closing_token.start(),
					format!("Missing terminating '{0}' after initial '{0}'", "%".fg(unit.colors().info))
				)
			);
			return
		}

		let varname = match VariableName::try_from(variable_name.as_str())
		{
			Ok(name) => name,
			Err(err) => {
				report_err!(
					unit,
					token.source(),
					"Invalid variable name".into(),
					span(
						variable_name.end()-1..closing_token.start(),
						err
					)
				);
				return
			},
		};

		let Some(variable) = unit.get_scope()
			.get_variable(&varname) else {
			report_err!(
				unit,
				token.source(),
				"Unknown variable".into(),
				span(
					variable_name.end()-1..closing_token.start(),
					format!("Unable to find a variable with name `{}`", &varname.0.fg(unit.colors().highlight))
				)
			);
			return
		};

		variable.0.expand(unit, token.clone());
	}
}
