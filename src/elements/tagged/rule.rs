use std::sync::Arc;
use std::{any::Any, ops::Range};

use crate::elements::lua::custom::LuaData;
use crate::elements::tagged::completion::TaggedCompletion;
use crate::elements::tagged::custom::{TaggedClosure, TaggedData, TaggedKind, TaggedProcessor};
use crate::lua::kernel::{Kernel, KernelContext, KernelNameBuf};
use crate::lua::wrappers::VecScopeProxy;
use crate::parser::reports::*;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;
use crate::parser::util::parse_paragraph;
use crate::parser::{reports::macros::*, util::escape_source};
use crate::unit::scope::Scope;
use crate::unit::translation::TranslationAccessors;

use ariadne::Fmt;
use parking_lot::RwLock;
use regex::Regex;
use rusqlite::params_from_iter;

use crate::{
	parser::{
		rule::{Rule, RuleTarget},
		source::Cursor,
		state::{CustomStates, ParseMode},
	},
	unit::translation::TranslationUnit,
};

#[auto_registry::auto_registry(registry = "rules")]
pub struct TaggedProcessorRule {
	re: [Regex; 1],
}

impl Default for TaggedProcessorRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r#"(?:^|\n)(:tagged)(?:\s+(\w+)?(?:\s+(\w+)(?:/(.*))?)?)?"#).unwrap()],
		}
	}
}

impl RegexRule for TaggedProcessorRule {
	fn name(&self) -> &'static str {
		"Tagged Processor"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Command
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
		_index: usize,
		unit: &mut TranslationUnit,
		token: Token,
		captures: regex::Captures,
	) {
		let Some(tag_name) = captures.get(2) else {
			report_err!(
				unit,
				token.source(),
				"Invalid Tagged Processor".into(),
				span(
					token.range.clone(),
					format!(
						"Expected tag name after {}",
						":tagged".fg(unit.colors().highlight)
					)
				)
			);
			return;
		};
		unit.with_lsp(|lsp| {
			lsp.with_semantics(token.source(), |sems, tokens| {
				// :tagged + name
				sems.add(captures.get(1).unwrap().range(), tokens.command);
				sems.add(tag_name.range(), tokens.tagged_proc_name);
			})
		});
		let Some(tagged_kind) = captures.get(3) else {
			report_err!(
				unit,
				token.source(),
				"Invalid Tagged Processor".into(),
				span(
					token.range.clone(),
					format!(
						"Expected tag kind after tag name {}",
						tag_name.as_str().fg(unit.colors().highlight)
					)
				)
			);
			return;
		};
		unit.with_lsp(|lsp| {
			lsp.with_semantics(token.source(), |sems, tokens| {
				// kind
				sems.add(tagged_kind.range(), tokens.tagged_proc_mode);
			})
		});
		let kind = match TaggedKind::try_from(tagged_kind.as_str()) {
			Ok(kind) => kind,
			Err(err) => {
				report_err!(
					unit,
					token.source(),
					"Invalid Tagged Processor".into(),
					span(token.range.clone(), err)
				);
				return;
			}
		};
		let Some(tagged_processor) = captures.get(4) else {
			report_err!(
				unit,
				token.source(),
				"Invalid Tagged Processor".into(),
				span(
					token.range.clone(),
					format!(
						"Expected tagged processor after tag kind {}",
						tagged_kind.as_str().fg(unit.colors().highlight)
					)
				)
			);
			return;
		};
		unit.with_lsp(|lsp| {
			lsp.with_semantics(token.source(), |sems, tokens| {
				sems.add(tagged_kind.end()..tagged_kind.end()+1, tokens.tagged_proc_sep);
				// sep + processor
				sems.add(tagged_processor.range(), tokens.tagged_proc_processor);
			})
		});

		LuaData::initialize(unit);
		LuaData::with_kernel(unit, &KernelNameBuf::new("main".into()), |unit, kernel| {
			let ctx = KernelContext::new(token.clone(), &kernel, unit);
			kernel.run_with_context(ctx, |ctx, lua| {
				let f: mlua::Function = match lua.globals().get(tagged_processor.as_str()) {
					Ok(f) => f,
					Err(err) => {
						report_err!(
							ctx.unit,
							token.source(),
							"Invalid Tagged Processor".into(),
							span(
								token.range.clone(),
								format!(
									"Failed to get Lua function {}: {err}",
									tagged_processor.as_str().fg(ctx.unit.colors().highlight)
								)
							)
						);
						return;
					}
				};
				let f_key: mlua::RegistryKey = match lua.create_registry_value(f) {
					Ok(key) => key,
					Err(err) => {
						report_err!(
							ctx.unit,
							token.source(),
							"Invalid Tagged Processor".into(),
							span(
								token.range.clone(),
								format!(
									"Failed to create Lua registry value from {}: {err}",
									tagged_processor.as_str().fg(ctx.unit.colors().highlight)
								)
							)
						);
						return;
					}
				};

				let tag_name = tag_name.as_str().to_string();
				let closure = match kind {
					TaggedKind::Raw => TaggedClosure::Raw(Arc::new(
						move |unit: &mut TranslationUnit,
						      token: Token,
						      ranges: Vec<Range<usize>>|
						      -> mlua::Result<()> {
							LuaData::with_kernel(
								unit,
								&KernelNameBuf::new("main".into()),
								|unit, kernel| {
									let ctx = KernelContext::new(token.clone(), &kernel, unit);
									kernel.run_with_context(ctx, |_ctx, lua| -> mlua::Result<()> {
										let f: mlua::Function = lua.registry_value(&f_key)?;
										let token_ud = lua.create_userdata(token)?;
										f.call((
											token_ud,
											mlua::LuaSerdeExt::to_value(lua, &ranges),
										))
									})
								},
							)
						},
					)),
					TaggedKind::Parsed => TaggedClosure::Parsed(Arc::new(
						move |unit: &mut TranslationUnit,
						      token: Token,
						      content: Vec<Arc<RwLock<Scope>>>| {
							LuaData::with_kernel(
								unit,
								&KernelNameBuf::new("main".into()),
								|unit, kernel| {
									let proxy = VecScopeProxy(&content as *const _);
									let ctx = KernelContext::new(token.clone(), &kernel, unit);
									kernel.run_with_context(ctx, |_ctx, lua| -> mlua::Result<()> {
										let f: mlua::Function = lua.registry_value(&f_key)?;
										let token_ud = lua.create_userdata(token)?;
										let content_ud = lua.create_userdata(proxy);
										f.call((token_ud, content_ud))
									})
								},
							)
						},
					)),
				};

				eprintln!("BEFORE");
				TaggedData::add_processor(ctx.unit, tag_name, TaggedProcessor { kind, closure });
				eprintln!("AFTER");
			})
		});
	}

	fn completion(
		&self,
	) -> Option<Box<dyn lsp::completion::CompletionProvider + 'static + Send + Sync>> {
		Some(Box::new(TaggedCompletion {}))
	}
}

#[auto_registry::auto_registry(registry = "rules")]
pub struct TaggedRule {
	start_re: Regex,
}

impl Default for TaggedRule {
	fn default() -> Self {
		Self {
			start_re: Regex::new(r#"\{@(\w+)"#).unwrap(),
		}
	}
}

impl Rule for TaggedRule {
	fn name(&self) -> &'static str {
		"Tagged"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Inline
	}

	fn next_match(
		&self,
		_unit: &TranslationUnit,
		_mode: &ParseMode,
		_states: &mut CustomStates,
		cursor: &Cursor,
	) -> Option<(Range<usize>, Box<dyn Any + Send + Sync>)> {
		self.start_re
			.find_at(cursor.source().content(), cursor.pos())
			.map(|m| {
				(
					m.range(),
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
		let content = source.content();
		let captures = self.start_re.captures_at(content, cursor.pos()).unwrap();
		assert_eq!(captures.get(0).unwrap().start(), cursor.pos());

		let tag = captures.get(1).unwrap();
		let Some(processor) = TaggedData::get_processor(unit, tag.as_str()) else {
			report_err!(
				unit,
				cursor.source(),
				"Invalid Tagged Content".into(),
				span(
					captures.get(1).unwrap().range(),
					format!(
						"Cannot find tag processor {}",
						tag.as_str().fg(unit.colors().highlight)
					)
				)
			);
			return cursor.at(captures.get(0).unwrap().end());
		};

		unit.with_lsp(|lsp| {
			lsp.with_semantics(cursor.source(), |sems, tokens| {
				// {@
				sems.add(
					captures.get(0).unwrap().start()..captures.get(0).unwrap().start() + 2,
					tokens.tagged_delim,
				);
				// tag
				sems.add(tag.range(), tokens.tagged_tag);
			})
		});

		let mut delims = vec![];
		let start = captures.get(0).unwrap().end();
		let mut last = start;
		let mut balance = 1;
		let mut escaped = false;
		for (i, ch) in content[start..].char_indices() {
			if escaped {
				escaped = false;
				continue;
			}
			escaped = ch == '\\';
			if ch == '{' {
				if balance == 1 {
					last = start + i + 1;
				}
				balance += 1;
			} else if ch == '}' {
				balance -= 1;
				if balance == 1 {
					delims.push(last..start + i);
				}
				if balance == 0 {
					last = start + i + 1;
					break;
				}
			}
		}
		if balance != 0 {
			report_err!(
				unit,
				cursor.source(),
				"Invalid Tagged Content".into(),
				span(
					captures.get(0).unwrap().start()..last,
					format!("Unmatched `{}`", "{".fg(unit.colors().highlight))
				)
			);
			return cursor.at(last);
		}
		// If empty, insert entire range
		if delims.is_empty() {
			delims.push(start..last - 1);
		}

		// Lsp
		for range in delims.iter() {
			// Start delim
			if content.as_bytes()[range.start - 1] == b'{' {
				unit.with_lsp(|lsp| {
					lsp.with_semantics(cursor.source(), |sems, tokens| {
						// {
						sems.add_to_queue(range.start - 1..range.start, tokens.tagged_delim);
					})
				});
			}

			// End delim
			if content.as_bytes()[range.end] == b'}' {
				unit.with_lsp(|lsp| {
					lsp.with_semantics(cursor.source(), |sems, tokens| {
						// {
						sems.add_to_queue(range.end..range.end + 1, tokens.tagged_delim);
					})
				});
			}
		}

		let token = Token::new(cursor.pos()..last, source.clone());
		match &processor.closure {
			TaggedClosure::Raw(f) => {
				if let Err(err) = (*f)(unit, token, delims) {
					report_err!(
						unit,
						source.clone(),
						"Invalid Tagged Content".into(),
						span(
							cursor.pos()..last,
							format!("Tagged processor failed:\n{err}")
						)
					);
				}
			}
			TaggedClosure::Parsed(f) => {
				let mut scopes = vec![];
				for (id, range) in delims.iter().enumerate() {
					let src = escape_source(
						source.clone(),
						range.clone(),
						format!("Tagged source#{id}").into(),
						'\\',
						"}",
					);

					match parse_paragraph(unit, src.clone()) {
						Ok(paragraph) => scopes.push(paragraph),
						Err(err) => {
							report_err!(
								unit,
								src.clone(),
								"Invalid Tagged Content".into(),
								span(
									0..src.content().len(),
									format!("Failed to parse tagged content:\n{err}")
								)
							);
							return cursor.at(last);
						}
					}
				}

				if let Err(err) = (*f)(unit, token, scopes) {
					report_err!(
						unit,
						source.clone(),
						"Invalid Tagged Content".into(),
						span(
							cursor.pos()..last,
							format!("Tagged processor failed:\n{err}")
						)
					);
				}
			}
		}

		// End delim
		unit.with_lsp(|lsp| {
			lsp.with_semantics(cursor.source(), |sems, tokens| {
				// }
				sems.add(last - 1..last, tokens.tagged_delim);
			})
		});
		cursor.at(last)
	}
}
