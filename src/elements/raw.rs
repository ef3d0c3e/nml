use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::lsp::semantic::Semantics;
use crate::lua::kernel::CTX;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;
use crate::parser::util::{self};
use ariadne::Fmt;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use parser::util::escape_source;
use regex::Captures;
use regex::Regex;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

#[derive(Debug)]
pub struct Raw {
	pub location: Token,
	pub kind: ElemKind,
	pub content: String,
}

impl Element for Raw {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { self.kind.clone() }

	fn element_name(&self) -> &'static str { "Raw" }

	fn compile(
		&self,
		_compiler: &Compiler,
		_document: &dyn Document,
		_cursor: usize,
	) -> Result<String, String> {
		Ok(self.content.clone())
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::raw")]
pub struct RawRule {
	re: [Regex; 1],
	properties: PropertyParser,
}

impl Default for RawRule {
	fn default() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"kind".to_string(),
			Property::new(
				"Element display kind".to_string(),
				Some("inline".to_string()),
			),
		);
		Self {
			re: [
				Regex::new(r"\{\?(?:\[((?:\\.|[^\[\]\\])*?)\])?(?:((?:\\.|[^\\\\])*?)(\?\}))?")
					.unwrap(),
			],
			properties: PropertyParser { properties: props },
		}
	}
}

impl RegexRule for RawRule {
	fn name(&self) -> &'static str { "Raw" }

	fn previous(&self) -> Option<&'static str> { Some("Variable Substitution") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn enabled(&self, _mode: &ParseMode, _id: usize) -> bool { true }

	fn on_regex_match(
		&self,
		_index: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		let raw_content = match matches.get(2) {
			// Unterminated
			None => {
				report_err!(
					&mut reports,
					token.source(),
					"Unterminated Raw Code".into(),
					span(
						token.range.clone(),
						format!(
							"Missing terminating `{}` after first `{}`",
							"?}".fg(state.parser.colors().info),
							"{?".fg(state.parser.colors().info)
						)
					)
				);
				return reports;
			}
			Some(content) => {
				let processed =
					util::escape_text('\\', "?}", content.as_str().trim_start().trim_end(), true);

				if processed.is_empty() {
					report_warn!(
						&mut reports,
						token.source(),
						"Empty Raw Code".into(),
						span(content.range(), "Raw code is empty".into())
					);
				}
				processed
			}
		};

		let prop_source = escape_source(
			token.source(),
			matches.get(1).map_or(0..0, |m| m.range()),
			"Raw Properties".into(),
			'\\',
			"]",
		);
		let properties = match self.properties.parse(
			"Raw Code",
			&mut reports,
			state,
			Token::new(0..prop_source.content().len(), prop_source),
		) {
			Some(props) => props,
			None => return reports,
		};

		let raw_kind = match properties.get(&mut reports, "kind", |_, value| {
			ElemKind::from_str(value.value.as_str())
		}) {
			None => return reports,
			Some(raw_kind) => raw_kind,
		};

		state.push(
			document,
			Box::new(Raw {
				location: token.clone(),
				kind: raw_kind,
				content: raw_content,
			}),
		);

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			let range = matches.get(0).unwrap().range();
			sems.add(range.start..range.start + 2, tokens.raw_sep);
			if let Some(props) = matches.get(1).map(|m| m.range()) {
				sems.add(props.start - 1..props.start, tokens.raw_props_sep);
				sems.add(props.end..props.end + 1, tokens.raw_props_sep);
			}
			sems.add(matches.get(2).unwrap().range(), tokens.raw_content);
			sems.add(range.end - 2..range.end, tokens.raw_sep);
		}

		reports
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];

		bindings.push((
			"push".to_string(),
			lua.create_function(|_, (kind, content): (String, String)| {
				// Validate kind
				let kind = match ElemKind::from_str(kind.as_str()) {
					Ok(kind) => kind,
					Err(e) => {
						return Err(BadArgument {
							to: Some("push".to_string()),
							pos: 1,
							name: Some("kind".to_string()),
							cause: Arc::new(mlua::Error::external(format!(
								"Wrong section kind specified: {e}"
							))),
						})
					}
				};

				CTX.with_borrow(|ctx| {
					ctx.as_ref().map(|ctx| {
						ctx.state.push(
							ctx.document,
							Box::new(Raw {
								location: ctx.location.clone(),
								kind,
								content,
							}),
						);
					})
				});

				Ok(())
			})
			.unwrap(),
		));

		bindings
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::elements::paragraph::Paragraph;
	use crate::elements::text::Text;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;
	use crate::validate_semantics;
	use std::rc::Rc;

	#[test]
	fn parser() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
Break{?[kind=block] Raw?}NewParagraph{?<b>?}
				"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(
			ParserState::new(&parser, None),
			source,
			None,
			ParseMode::default(),
		);

		validate_document!(doc.content().borrow(), 0,
			Paragraph;
			Raw { kind == ElemKind::Block, content == "Raw" };
			Paragraph {
				Text;
				Raw { kind == ElemKind::Inline, content == "<b>" };
			};
		);
	}

	#[test]
	fn lua() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
Break%<nml.raw.push("block", "Raw")>%NewParagraph%<nml.raw.push("inline", "<b>")>%
				"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(
			ParserState::new(&parser, None),
			source,
			None,
			ParseMode::default(),
		);

		validate_document!(doc.content().borrow(), 0,
		Paragraph;
		Raw { kind == ElemKind::Block, content == "Raw" };
		Paragraph {
			Text;
			Raw { kind == ElemKind::Inline, content == "<b>" };
		};
		);
	}

	#[test]
	fn semantic() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
{?[kind=block] Raw?}
{?<b>?}
		"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (_, state) = parser.parse(
			ParserState::new_with_semantics(&parser, None),
			source.clone(),
			None,
			ParseMode::default(),
		);
		validate_semantics!(state, source.clone(), 0,
			raw_sep { delta_line == 1, delta_start == 0, length == 2 };
			raw_props_sep { delta_line == 0, delta_start == 2, length == 1 };
			prop_name { delta_line == 0, delta_start == 1, length == 4 };
			prop_equal { delta_line == 0, delta_start == 4, length == 1 };
			prop_value { delta_line == 0, delta_start == 1, length == 5 };
			raw_props_sep { delta_line == 0, delta_start == 5, length == 1 };
			raw_content { delta_line == 0, delta_start == 1, length == 4 };
			raw_sep { delta_line == 0, delta_start == 4, length == 2 };
			raw_sep { delta_line == 1, delta_start == 0, length == 2 };
			raw_content { delta_line == 0, delta_start == 2, length == 3 };
			raw_sep { delta_line == 0, delta_start == 3, length == 2 };
		);
	}
}
