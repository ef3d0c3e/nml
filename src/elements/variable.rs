use crate::document::document::Document;
use crate::document::variable::BaseVariable;
use crate::document::variable::PathVariable;
use crate::document::variable::Variable;
use crate::lsp::semantic::Semantics;
use crate::lua::kernel::CTX;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::parser::ReportColors;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;
use ariadne::Fmt;
use lsp::definition;
use lsp::hints::Hints;
use mlua::Function;
use mlua::Lua;
use regex::Regex;
use std::rc::Rc;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VariableKind {
	Regular,
	Path,
}

impl FromStr for VariableKind {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"regular" | "" => Ok(VariableKind::Regular),
			"path" | "'" => Ok(VariableKind::Path),
			_ => Err(format!("Uknnown variable kind: `{s}`")),
		}
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::variable")]
pub struct VariableRule {
	re: [Regex; 1],
	kinds: Vec<(String, String)>,
}

impl VariableRule {
	pub fn new() -> Self {
		Self {
			re: [Regex::new(r"(?:^|\n)@(')?(.*?)=((?:\\\n|.)*)").unwrap()],
			kinds: vec![("".into(), "Regular".into()), ("'".into(), "Path".into())],
		}
	}

	pub fn make_variable(
		&self,
		colors: &ReportColors,
		location: Token,
		kind: usize,
		name: String,
		value: String,
	) -> Result<Rc<dyn Variable>, String> {
		match self.kinds[kind].0.as_str() {
			"" => Ok(Rc::new(BaseVariable::new(location, name, value))),
			"'" => {
				match std::fs::canonicalize(value.as_str()) // TODO: not canonicalize
                {
                    Ok(path) => Ok(Rc::new(PathVariable::new(location, name, path))),
                    Err(e) => Err(format!("Unable to canonicalize path `{}`: {}",
                            value.fg(colors.highlight),
                            e))
                }
			}
			_ => panic!("Unhandled variable kind"),
		}
	}

	// Trim and check variable name for validity
	pub fn validate_name<'a>(
		colors: &ReportColors,
		original_name: &'a str,
	) -> Result<&'a str, String> {
		let name = original_name.trim_start().trim_end();
		if name.contains("%") {
			return Err(format!("Name cannot contain '{}'", "%".fg(colors.info)));
		}
		Ok(name)
	}

	pub fn validate_value(original_value: &str) -> Result<String, String> {
		let mut escaped = 0usize;
		let mut result = String::new();
		for c in original_value.trim_start().trim_end().chars() {
			if c == '\\' {
				escaped += 1
			} else if c == '\n' {
				match escaped {
					0 => return Err("Unknown error wile capturing value".to_string()),
					// Remove '\n'
					1 => {}
					// Insert '\n'
					_ => {
						result.push(c);
						(0..escaped - 2).for_each(|_| result.push('\\'));
					}
				}
				escaped = 0;
			} else {
				(0..escaped).for_each(|_| result.push('\\'));
				escaped = 0;
				result.push(c);
			}
		}
		(0..escaped).for_each(|_| result.push('\\'));

		Ok(result)
	}
}

impl RegexRule for VariableRule {
	fn name(&self) -> &'static str { "Variable" }

	fn previous(&self) -> Option<&'static str> { Some("Element Style") }

	fn regexes(&self) -> &[Regex] { &self.re }

	fn enabled(&self, mode: &ParseMode, _id: usize) -> bool { !mode.paragraph_only }

	fn on_regex_match(
		&self,
		_: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: regex::Captures,
	) -> Vec<Report> {
		let mut reports = vec![];
		// [Optional] variable kind
		let var_kind = match matches.get(1) {
			Some(kind) => {
				// Find kind
				let r = self
					.kinds
					.iter()
					.enumerate()
					.find(|(_i, (ref char, ref _name))| char == kind.as_str());

				// Unknown kind specified
				if r.is_none() {
					report_err!(
						&mut reports,
						token.source(),
						"Unknowm Variable Kind".into(),
						span(
							kind.range(),
							format!(
								"Variable kind `{}` is unknown",
								kind.as_str().fg(state.parser.colors().highlight)
							)
						),
						help(format!(
							"Leave empty for regular variables. Available variable kinds:{}",
							self.kinds
								.iter()
								.skip(1)
								.fold("".to_string(), |acc, (char, name)| {
									acc + format!(
										"\n - `{}` : {}",
										char.fg(state.parser.colors().highlight),
										name.fg(state.parser.colors().info)
									)
									.as_str()
								})
						))
					);
					return reports;
				}

				r.unwrap().0
			}
			None => 0,
		};

		let var_name = match matches.get(2) {
			Some(name) => match VariableRule::validate_name(state.parser.colors(), name.as_str()) {
				Ok(var_name) => var_name,
				Err(msg) => {
					report_err!(
						&mut reports,
						token.source(),
						"Invalid Variable Name".into(),
						span(
							name.range(),
							format!(
								"Variable name `{}` is not allowed. {msg}",
								name.as_str().fg(state.parser.colors().highlight)
							)
						),
					);

					return reports;
				}
			},
			_ => panic!("Unknown variable name"),
		};

		let var_value = match matches.get(3) {
			Some(value) => match VariableRule::validate_value(value.as_str()) {
				Ok(var_value) => var_value,
				Err(msg) => {
					report_err!(
						&mut reports,
						token.source(),
						"Invalid Variable Value".into(),
						span(
							value.range(),
							format!(
								"Variable value `{}` is not allowed. {msg}",
								value.as_str().fg(state.parser.colors().highlight)
							)
						),
					);

					return reports;
				}
			},
			_ => panic!("Invalid variable value"),
		};

		match self.make_variable(
			state.parser.colors(),
			token.clone(),
			var_kind,
			var_name.to_string(),
			var_value,
		) {
			Ok(variable) => document.add_variable(variable),
			Err(msg) => {
				let m = matches.get(0).unwrap();
				report_err!(
					&mut reports,
					token.source(),
					"Unable to Create Variable".into(),
					span(
						m.start() + 1..m.end(),
						format!(
							"Unable to create variable `{}`. {}",
							var_name.fg(state.parser.colors().highlight),
							msg
						)
					),
				);

				return reports;
			}
		}

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			let name = matches.get(2).unwrap().range();
			if let Some(kind) = matches.get(1).map(|m| m.range()) {
				sems.add(kind.start - 1..kind.start, tokens.variable_operator);
				sems.add(kind, tokens.variable_kind);
			} else {
				sems.add(name.start - 1..name.start, tokens.variable_operator);
			}
			sems.add(name.clone(), tokens.variable_name);
			sems.add(name.end..name.end + 1, tokens.variable_sep);
			let value = matches.get(3).unwrap().range();
			sems.add(value.clone(), tokens.variable_value);
		}

		reports
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];
		bindings.push((
			"insert".to_string(),
			lua.create_function(|_, (name, value): (String, String)| {
				CTX.with_borrow(|ctx| {
					ctx.as_ref().map(|ctx| {
						let var = Rc::new(BaseVariable::new(ctx.location.clone(), name, value));
						ctx.document.add_variable(var);
					})
				});

				Ok(())
			})
			.unwrap(),
		));
		bindings.push((
			"get".to_string(),
			lua.create_function(|_, name: String| {
				let mut value: Option<String> = None;
				CTX.with_borrow(|ctx| {
					ctx.as_ref().map(|ctx| {
						if let Some(var) = ctx.document.get_variable(name.as_str()) {
							value = Some(var.to_string());
						}
					})
				});

				Ok(value)
			})
			.unwrap(),
		));

		bindings
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::variable")]
pub struct VariableSubstitutionRule {
	re: [Regex; 1],
}

impl VariableSubstitutionRule {
	pub fn new() -> Self {
		Self {
			re: [Regex::new(r"%(.*?)%").unwrap()],
		}
	}
}

impl RegexRule for VariableSubstitutionRule {
	fn name(&self) -> &'static str { "Variable Substitution" }

	fn previous(&self) -> Option<&'static str> { Some("Variable") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn enabled(&self, _mode: &ParseMode, _id: usize) -> bool { true }

	fn on_regex_match<'a>(
		&self,
		_index: usize,
		state: &ParserState,
		document: &'a dyn Document<'a>,
		token: Token,
		matches: regex::Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		let variable = match matches.get(1) {
			Some(name) => {
				// Empty name
				if name.as_str().is_empty() {
					report_err!(
						&mut reports,
						token.source(),
						"Empty Variable Name".into(),
						span(
							name.range(),
							"Missing variable name for substitution".into()
						)
					);

					return reports;
				}
				// Leading spaces
				else if name.as_str().trim_start() != name.as_str() {
					report_err!(
						&mut reports,
						token.source(),
						"Invalid Variable Name".into(),
						span(
							name.range(),
							"Variable names contains leading spaces".into()
						),
						help("Remove leading spaces".into())
					);

					return reports;
				}
				// Trailing spaces
				else if name.as_str().trim_end() != name.as_str() {
					report_err!(
						&mut reports,
						token.source(),
						"Invalid Variable Name".into(),
						span(
							name.range(),
							"Variable names contains trailing spaces".into()
						),
						help("Remove trailing spaces".into())
					);

					return reports;
				}
				// Invalid name
				if let Err(msg) = VariableRule::validate_name(state.parser.colors(), name.as_str())
				{
					report_err!(
						&mut reports,
						token.source(),
						"Invalid Variable Name".into(),
						span(name.range(), msg)
					);

					return reports;
				}

				// Get variable
				match document.get_variable(name.as_str()) {
					None => {
						report_err!(
							&mut reports,
							token.source(),
							"Unknown Variable Name".into(),
							span(
								name.range(),
								format!(
									"Unable to find variable with name: `{}`",
									name.as_str().fg(state.parser.colors().highlight)
								)
							)
						);
						return reports;
					}
					Some(var) => var,
				}
			}
			_ => panic!("Unknown error"),
		};

		variable.parse(state, token.clone(), document);

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			let name = matches.get(1).unwrap().range();
			sems.add(name.start - 1..name.start, tokens.variable_sub_sep);
			sems.add(name.clone(), tokens.variable_sub_name);
			sems.add(name.end..name.end + 1, tokens.variable_sub_sep);
		}

		if let Some(hints) = Hints::from_source(token.source(), &state.shared.lsp) {
			let label = variable.to_string();
			if !label.is_empty() {
				hints.add(matches.get(0).unwrap().end(), label);
			}
		}

		// Add definition
		definition::from_source(token, variable.location(), &state.shared.lsp);

		reports
	}
}
