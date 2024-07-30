use crate::document::document::Document;
use crate::document::variable::BaseVariable;
use crate::document::variable::PathVariable;
use crate::document::variable::Variable;
use crate::lua::kernel::CTX;
use crate::parser::parser::Parser;
use crate::parser::parser::ReportColors;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use mlua::Function;
use mlua::Lua;
use regex::Regex;
use std::ops::Range;
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

pub struct VariableRule {
	re: [Regex; 1],
	kinds: Vec<(String, String)>,
}

impl VariableRule {
	pub fn new() -> Self {
		Self {
			re: [Regex::new(r"(?:^|\n)@([^[:alpha:]])?(.*?)=((?:\\\n|.)*)").unwrap()],
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
                            e.to_string()))
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
		return Ok(name);
	}

	pub fn validate_value(_colors: &ReportColors, original_value: &str) -> Result<String, String> {
		let mut escaped = 0usize;
		let mut result = String::new();
		for c in original_value.trim_start().trim_end().chars() {
			if c == '\\' {
				escaped += 1
			} else if c == '\n' {
				match escaped {
					0 => return Err("Unknown error wile capturing variable".to_string()),
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

	fn regexes(&self) -> &[Regex] { &self.re }

	fn on_regex_match<'a>(
		&self,
		_: usize,
		parser: &dyn Parser,
		document: &'a dyn Document,
		token: Token,
		matches: regex::Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut result = vec![];
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
					result.push(
						Report::build(ReportKind::Error, token.source(), kind.start())
							.with_message("Unknown variable kind")
							.with_label(
								Label::new((token.source(), kind.range()))
									.with_message(format!(
										"Variable kind `{}` is unknown",
										kind.as_str().fg(parser.colors().highlight)
									))
									.with_color(parser.colors().error),
							)
							.with_help(format!(
								"Leave empty for regular variables. Available variable kinds:{}",
								self.kinds.iter().skip(1).fold(
									"".to_string(),
									|acc, (char, name)| {
										acc + format!(
											"\n - `{}` : {}",
											char.fg(parser.colors().highlight),
											name.fg(parser.colors().info)
										)
										.as_str()
									}
								)
							))
							.finish(),
					);

					return result;
				}

				r.unwrap().0
			}
			None => 0,
		};

		let var_name = match matches.get(2) {
			Some(name) => match VariableRule::validate_name(&parser.colors(), name.as_str()) {
				Ok(var_name) => var_name,
				Err(msg) => {
					result.push(
						Report::build(ReportKind::Error, token.source(), name.start())
							.with_message("Invalid variable name")
							.with_label(
								Label::new((token.source(), name.range()))
									.with_message(format!(
										"Variable name `{}` is not allowed. {msg}",
										name.as_str().fg(parser.colors().highlight)
									))
									.with_color(parser.colors().error),
							)
							.finish(),
					);

					return result;
				}
			},
			_ => panic!("Unknown variable name"),
		};

		let var_value = match matches.get(3) {
			Some(value) => match VariableRule::validate_value(&parser.colors(), value.as_str()) {
				Ok(var_value) => var_value,
				Err(msg) => {
					result.push(
						Report::build(ReportKind::Error, token.source(), value.start())
							.with_message("Invalid variable value")
							.with_label(
								Label::new((token.source(), value.range()))
									.with_message(format!(
										"Variable value `{}` is not allowed. {msg}",
										value.as_str().fg(parser.colors().highlight)
									))
									.with_color(parser.colors().error),
							)
							.finish(),
					);

					return result;
				}
			},
			_ => panic!("Invalid variable value"),
		};

		match self.make_variable(
			&parser.colors(),
			token.clone(),
			var_kind,
			var_name.to_string(),
			var_value,
		) {
			Ok(variable) => document.add_variable(variable),
			Err(msg) => {
				let m = matches.get(0).unwrap();
				result.push(
					Report::build(ReportKind::Error, token.source(), m.start())
						.with_message("Unable to create variable")
						.with_label(
							Label::new((token.source(), m.start() + 1..m.end()))
								.with_message(format!(
									"Unable to create variable `{}`. {}",
									var_name.fg(parser.colors().highlight),
									msg
								))
								.with_color(parser.colors().error),
						)
						.finish(),
				);

				return result;
			}
		}

		return result;
	}

	fn lua_bindings<'lua>(&self, lua: &'lua Lua) -> Option<Vec<(String, Function<'lua>)>> {
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
						if let Some(var) = ctx.document.get_variable(name.as_str())
						{
							value = Some(var.to_string());
						}
					})
				});

				Ok(value)
			})
			.unwrap(),
		));

		Some(bindings)
	}
}

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

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn on_regex_match<'a>(
		&self,
		_index: usize,
		parser: &dyn Parser,
		document: &'a dyn Document<'a>,
		token: Token,
		matches: regex::Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut result = vec![];

		let variable = match matches.get(1) {
			Some(name) => {
				// Empty name
				if name.as_str().is_empty() {
					result.push(
						Report::build(ReportKind::Error, token.source(), name.start())
							.with_message("Empty variable name")
							.with_label(
								Label::new((token.source(), matches.get(0).unwrap().range()))
									.with_message(format!("Missing variable name for substitution"))
									.with_color(parser.colors().error),
							)
							.finish(),
					);

					return result;
				}
				// Leading spaces
				else if name.as_str().trim_start() != name.as_str() {
					result.push(
						Report::build(ReportKind::Error, token.source(), name.start())
							.with_message("Invalid variable name")
							.with_label(
								Label::new((token.source(), name.range()))
									.with_message(format!("Variable names contains leading spaces"))
									.with_color(parser.colors().error),
							)
							.with_help("Remove leading spaces")
							.finish(),
					);

					return result;
				}
				// Trailing spaces
				else if name.as_str().trim_end() != name.as_str() {
					result.push(
						Report::build(ReportKind::Error, token.source(), name.start())
							.with_message("Invalid variable name")
							.with_label(
								Label::new((token.source(), name.range()))
									.with_message(format!(
										"Variable names contains trailing spaces"
									))
									.with_color(parser.colors().error),
							)
							.with_help("Remove trailing spaces")
							.finish(),
					);

					return result;
				}
				// Invalid name
				match VariableRule::validate_name(&parser.colors(), name.as_str()) {
					Err(msg) => {
						result.push(
							Report::build(ReportKind::Error, token.source(), name.start())
								.with_message("Invalid variable name")
								.with_label(
									Label::new((token.source(), name.range()))
										.with_message(msg)
										.with_color(parser.colors().error),
								)
								.finish(),
						);

						return result;
					}
					_ => {}
				}

				// Get variable
				match document.get_variable(name.as_str()) {
					None => {
						result.push(
							Report::build(ReportKind::Error, token.source(), name.start())
								.with_message("Unknown variable name")
								.with_label(
									Label::new((token.source(), name.range()))
										.with_message(format!(
											"Unable to find variable with name: `{}`",
											name.as_str().fg(parser.colors().highlight)
										))
										.with_color(parser.colors().error),
								)
								.finish(),
						);
						return result;
					}
					Some(var) => var,
				}
			}
			_ => panic!("Unknown error"),
		};

		variable.parse(token, parser, document);

		return result;
	}

	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Option<Vec<(String, Function<'lua>)>> { None }
}
