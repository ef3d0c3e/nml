use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use regex::Regex;

use crate::lua::kernel;
use crate::lua::kernel::KernelContext;
use crate::lua::kernel::KernelName;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
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
	) -> Option<(usize, Box<dyn Any>)> {
		if mode.paragraph_only {
			return None;
		}

		self.start_re
			.find_at(cursor.source().content(), cursor.pos())
			.map(|m| (m.start(), Box::new([false; 0]) as Box<dyn Any>))
	}

	fn on_match<'u>(
		&self,
		unit: &mut TranslationUnit<'u>,
		cursor: &Cursor,
		_match_data: Box<dyn Any>,
	) -> Cursor {
		let source = cursor.source();
		let captures = self
			.start_re
			.captures_at(source.content(), cursor.pos())
			.unwrap();
		let end_cursor = cursor.at(captures.get(0).unwrap().end());

		let prop_source = escape_source(
			source.clone(),
			captures.get(2).unwrap().range(),
			"Lua Properties".into(),
			'\\',
			"",
		);

		unit.with_lsp(|lsp| {
			lsp.with_semantics(source.clone(), |sems, tokens| {
				sems.add(captures.get(1).unwrap().range(), tokens.command);
			})
		});

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
		let lua_content = Token::new(end_cursor.pos()..content_end, cursor.source());
		unit.with_lsp(|lsp| {
			lsp.with_semantics(source.clone(), |sems, tokens| {
				sems.add(lua_content.range.clone(), tokens.lua_content);
				sems.add(
					content_end..content_end + delimiter.len(),
					tokens.lua_delimiter,
				);
			})
		});
		let lua_source = Arc::new(VirtualSource::new(
			lua_content.clone(),
			format!(":LUA:Block({}..{})", lua_content.start(), lua_content.end()),
			lua_content.content().to_owned(),
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
					lua_content.source(),
					"Lua error".into(),
					span(lua_content.range.clone(), err.to_string())
				);
			}
		});

		unit.with_lsp(|lsp| {
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

	fn completion(
		&self,
	) -> Option<Box<dyn lsp::completion::CompletionProvider + 'static + Send + Sync>> {
		Some(Box::new(LuaCompletion {}))
	}
}
