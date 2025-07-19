use std::any::Any;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;

use ariadne::Span;
use regex::Captures;
use regex::Regex;

use crate::elements::meta::scope::ScopeElement;
use crate::elements::text::elem::Text;
use crate::lsp::ranges::CustomRange;
use crate::lua::kernel::Kernel;
use crate::lua::kernel::KernelContext;
use crate::lua::kernel::KernelName;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::rule::RegexRule;
use crate::parser::rule::Rule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::Cursor;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::parser::util::escape_source;
use crate::report_err;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use ariadne::Fmt;

use super::completion::LuaCompletion;
use super::custom::LuaData;
use super::elem::LuaEvalKind;
use super::elem::LuaPostProcess;

#[auto_registry::auto_registry(registry = "rules")]
pub struct LuaRule {
	start_re: Regex,
	properties: PropertyParser,
}

impl Default for LuaRule {
	fn default() -> Self {
		let mut properties = HashMap::new();
		properties.insert(
			"kernel".to_string(),
			Property::new("Lua kernel".to_string(), Some("main".to_string())),
		);
		properties.insert(
			"delim".to_string(),
			Property::new("Lua code delimiter".to_string(), Some("EOF".to_string())),
		);
		LuaRule {
			start_re: Regex::new(r"(?:\n|^)(:lua)([^\S\n\r]+.*|.?)?(?:\n|$)").unwrap(),
			properties: PropertyParser { properties },
		}
	}
}

impl Rule for LuaRule {
	fn name(&self) -> &'static str {
		"Lua"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Command
	}

	fn next_match(
		&self,
		_unit: &TranslationUnit,
		mode: &ParseMode,
		_states: &mut CustomStates,
		cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any + Send + Sync>)> {
		if mode.paragraph_only {
			return None;
		}

		self.start_re
			.find_at(cursor.source().content(), cursor.pos())
			.map(|m| {
				(
					m.start(),
					Box::new([false; 0]) as Box<dyn Any + Send + Sync>,
				)
			})
	}

	fn on_match<'u>(
		&self,
		unit: &mut TranslationUnit,
		cursor: &Cursor,
		_match_data: Box<dyn Any + Send + Sync>,
	) -> Cursor {
		let source = cursor.source();
		let captures = self
			.start_re
			.captures_at(source.content(), cursor.pos())
			.unwrap();
		let end_cursor = cursor.at(captures.get(0).unwrap().end());

		unit.with_lsp(|lsp| {
			lsp.with_semantics(source.clone(), |sems, tokens| {
				sems.add(captures.get(1).unwrap().range(), tokens.command);
			})
		});

		// Parse properties
		let prop_source = escape_source(
			source.clone(),
			captures.get(2).unwrap().range(),
			"Lua Properties".into(),
			'\\',
			"",
		);
		let Some(mut properties) = self.properties.parse("Lua", unit, prop_source.into()) else {
			return end_cursor;
		};
		let Some(kernel_name) = properties.get_or(
			unit,
			"kernel",
			KernelName("main".to_string()),
			|_, value| KernelName::try_from(value.value),
		) else {
			return end_cursor;
		};
		let Some(delimiter) = properties.get(unit, "delim", |_, value| {
			Result::<_, String>::Ok(value.value)
		}) else {
			return end_cursor;
		};

		// Find content
		let mut content_end = end_cursor.pos();
		loop {
			let Some(newline) = source.content()[content_end..].find(|c| c == '\n') else {
				report_err!(
					unit,
					source.clone(),
					"Unterminated Lua code".into(),
					span(
						captures.get(1).unwrap().start()..captures.get(2).unwrap().end(),
						format!(
							"Failed to find delimiter `{}`",
							delimiter.fg(unit.colors().highlight)
						)
					)
				);
				return end_cursor;
			};
			content_end += newline + 1;
			if source.content()[content_end..].starts_with(delimiter.as_str()) {
				break;
			}
		}
		let lua_range = Token::new(end_cursor.pos()..content_end, cursor.source());
		unit.with_lsp(|lsp| {
			lsp.with_semantics(source.clone(), |sems, tokens| {
				sems.add(lua_range.range.clone(), tokens.lua_content);
				sems.add(
					content_end..content_end + delimiter.len(),
					tokens.lua_delimiter,
				);
			})
		});
		let lua_source = Arc::new(VirtualSource::new(
			lua_range.clone(),
			format!(":LUA:Block ({}..{})", lua_range.start(), lua_range.end()),
			lua_range.content().to_owned(),
		)) as Arc<dyn Source>;

		// Evaluate
		LuaData::initialize(unit);
		LuaData::with_kernel(unit, &kernel_name, |unit, kernel| {
			let ctx = KernelContext::new(lua_source.clone().into(), unit);
			if let Err(err) = kernel.run_with_context(ctx, |lua| {
				lua.load(lua_source.content())
					.set_name(lua_source.name())
					.exec()
			}) {
				report_err!(
					unit,
					lua_range.source(),
					"Lua error".into(),
					span(lua_range.range.clone(), err.to_string())
				);
			}
		});

		unit.with_lsp(|lsp| {
			lsp.add_range(end_cursor.source(), lua_range.range, CustomRange::Lua);
			lsp.add_hover(
				Token::new(cursor.pos()..content_end + delimiter.len(), cursor.source()),
				format!(
					"# Lua block

 * **Kernel**: `{}`
 * **Delimiter**: `{delimiter}`
",
					&kernel_name.0
				),
			)
		});

		//let content =
		end_cursor.at(content_end + delimiter.len())
	}

	fn register_bindings(&self) {
	}

	fn completion(
		&self,
	) -> Option<Box<dyn lsp::completion::CompletionProvider + 'static + Send + Sync>> {
		Some(Box::new(LuaCompletion {}))
	}
}

#[auto_registry::auto_registry(registry = "rules")]
pub struct InlineLuaRule {
	re: [Regex; 2],
	properties: PropertyParser,
}

impl Default for InlineLuaRule {
	fn default() -> Self {
		let mut properties = HashMap::new();
		properties.insert(
			"kernel".to_string(),
			Property::new("Lua kernel".to_string(), Some("main".to_string())),
		);
		InlineLuaRule {
			re: [Regex::new(
				r"(\{:lua)(?:\[((?:\\.|[^\\\\])*?)\])?(!|')?\s(?:((?:\\.|[^\\\\])*?)(:\}))?",
			)
			.unwrap(),
			Regex::new(
				r"(\{:lua_post)(?:\[((?:\\.|[^\\\\])*?)\])?(!|')?\s(?:((?:\\.|[^\\\\])*?)(:\}))?",
			)
				.unwrap()
			],
			properties: PropertyParser { properties },
		}
	}
}

impl RegexRule for InlineLuaRule {
	fn name(&self) -> &'static str {
		"Inline Lua"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Inline
	}

	fn regexes(&self) -> &[regex::Regex] {
		&self.re
	}

	fn enabled(
		&self,
		_unit: &TranslationUnit,
		mode: &ParseMode,
		_states: &mut CustomStates,
		_index: usize,
	) -> bool {
		!mode.paragraph_only
	}

	fn on_regex_match<'u>(
		&self,
		index: usize,
		unit: &mut TranslationUnit,
		token: Token,
		captures: Captures,
	) {
		unit.with_lsp(|lsp| {
			lsp.with_semantics(token.source(), |sems, tokens| {
				sems.add(captures.get(1).unwrap().range(), tokens.lua_delimiter);
				if let Some(range) = captures.get(2).map(|m| m.range()) {
					sems.add(range.start() - 1..range.start(), tokens.lua_prop_sep);
					sems.add_to_queue(range.end()..range.end() + 1, tokens.lua_prop_sep);
				}
				if let Some(kind) = captures.get(3).map(|m| m.range()) {
					sems.add_to_queue(kind, tokens.lua_kind);
				}
			})
		});

		if captures.get(5).is_none() {
			report_err!(
				unit,
				token.source(),
				"Invalid inline Lua".into(),
				span(
					token.range.clone(),
					format!(
						"Expected terminating `{}`",
						":}".fg(unit.colors().highlight)
					)
				)
			);
			return;
		}

		// Parse properties
		let prop_source = escape_source(
			token.source(),
			captures.get(2).map_or(0..0, |m| m.range()),
			"Properties for Lua".into(),
			'\\',
			"]",
		);
		let Some(mut properties) = self.properties.parse("Lua", unit, prop_source.into()) else {
			return;
		};
		let Some(kernel_name) = properties.get_or(
			unit,
			"kernel",
			KernelName("main".to_string()),
			|_, value| KernelName::try_from(value.value),
		) else {
			return;
		};

		// Get evaluation kind
		let eval_kind = LuaEvalKind::from_str(captures.get(3).map_or("", |m| m.as_str())).unwrap();

		// Get content
		let lua_range = captures.get(4).unwrap().range();
		let lua_source = escape_source(
			token.source(),
			lua_range.clone(),
			format!(":LUA:Inline ({}..{})", lua_range.start(), lua_range.end()),
			'\\',
			":}",
		);

		unit.with_lsp(|lsp| {
			lsp.with_semantics(token.source(), |sems, tokens| {
				sems.add(lua_range.clone(), tokens.lua_content);
				sems.add(
					token.range.end() - 2..token.range.end(),
					tokens.lua_delimiter,
				);
			})
		});

		// Add lua post process task
		if index == 1
		{
			unit.add_content(Arc::new(LuaPostProcess{ location: token.clone(), expanded: OnceLock::new(), source: lua_source, kernel_name, eval_kind }));
			return
		}

		// Evaluate
		LuaData::initialize(unit);
		LuaData::with_kernel(unit, &kernel_name, |unit, kernel| {
			let parsed = unit.with_child(
				lua_source.clone(),
				ParseMode::default(),
				true,
				|unit, scope| {
					let ctx = KernelContext::new(lua_source.clone().into(), unit);
					match kernel.run_with_context(ctx, |lua| match eval_kind {
						LuaEvalKind::None => lua
							.load(lua_source.content())
							.set_name(lua_source.name())
							.eval::<()>()
							.map(|_| String::default()),
						LuaEvalKind::String | LuaEvalKind::StringParse => lua
							.load(lua_source.content())
							.set_name(lua_source.name())
							.eval::<String>(),
						_ => panic!(),
					}) {
						Err(err) => {
							report_err!(
								unit,
								token.source(),
								"Lua Error".into(),
								span(lua_range.clone(), err.to_string())
							);
						}
						Ok(result) => {
							if eval_kind == LuaEvalKind::String && !result.is_empty() {
								unit.add_content(Arc::new(Text {
									location: lua_source.into(),
									content: result,
								}));
							} else if eval_kind == LuaEvalKind::StringParse && !result.is_empty() {
								let content = Arc::new(VirtualSource::new(
									token.clone(),
									":LUA:Inline lua result".into(),
									result,
								));
								let mode = unit.get_scope().read().parser_state().mode.clone();
								let scope = unit.with_child(
									content as Arc<dyn Source>,
									mode,
									true,
									|unit, scope| {
										unit.parser.clone().parse(unit);
										scope
									},
								);
								unit.add_content(Arc::new(ScopeElement {
									token: lua_source.into(),
									scope: [scope],
								}));
							}
						}
					}
					scope
				},
			);
			unit.add_content(Arc::new(ScopeElement {
				token,
				scope: [parsed],
			}));
		});
	}
}
