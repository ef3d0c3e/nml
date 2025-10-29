use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use ariadne::Fmt;
use mlua::LuaSerdeExt;
use regex::Regex;

use crate::lua::wrappers::*;
use crate::lua::kernel::Kernel;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::Rule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::Cursor;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::parser::util::escape_source;
use crate::parser::util::parse_paragraph;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

use super::completion::ListCompletion;
use super::elem::BulletMarker;
use super::elem::CheckboxState;
use super::elem::List;
use super::elem::ListEntry;
use super::elem::ListMarker;

#[auto_registry::auto_registry(registry = "rules")]
pub struct ListRule {
	start_re: Regex,
	continue_re: Regex,
	properties: PropertyParser,
}

impl Default for ListRule {
	fn default() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"offset".to_string(),
			Property::new("Entry numbering offset".to_string(), None),
		);

		Self {
			start_re: Regex::new(r"(?:^|\n)(?:[^\S\r\n]+)([*-]+)(?:\[((?:\\.|[^\\\\])*?)\])?(?:[^\S\r\n]{0,1}\[((?:\\.|[^\\\\])*?)\])?(?:[^\S\r\n]+)(.*)")
				.unwrap(),
			continue_re: Regex::new(r"(?:^|\n)([^\S\r\n].*)").unwrap(),
			properties: PropertyParser { properties: props },
		}
	}
}

impl Rule for ListRule {
	fn name(&self) -> &'static str {
		"List"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Block
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
		let content = source.content();
		let mut end_cursor = cursor.clone();
		let mut list = List {
			location: Token::new(0..0, cursor.source()),
			contained: vec![],
			entries: vec![],
		};

		let parse_depth =
			|depth: &str, offset: usize, previous: &Vec<ListMarker>| -> Vec<ListMarker> {
				// Build vec
				let mut parsed = depth
					.chars()
					.map(|c| match c {
						'*' => ListMarker {
							numbered: false,
							offset: 0,
						},
						'-' => ListMarker {
							numbered: true,
							offset: 0,
						},
						_ => panic!(),
					})
					.collect::<Vec<_>>();

				// Appply offsets
				let mut matched = true;
				for i in 0..parsed.len() {
					if matched
						&& previous
							.get(i)
							.is_some_and(|prev| prev.numbered == parsed[i].numbered)
					{
						if i + 1 == parsed.len() {
							parsed[i].offset = 1;
						}
					} else {
						matched = false;
						parsed[i].offset = 1;
					}
				}
				parsed.last_mut().map(|last| last.offset = offset);

				parsed
			};

		let mut parse_entry = || -> bool {
			let Some(captures) = self.start_re.captures_at(content, end_cursor.pos()) else {
				return false;
			};
			if captures.get(0).unwrap().start() != end_cursor.pos() {
				return false;
			}
			end_cursor = end_cursor.at(captures.get(0).unwrap().end());

			// Semantic
			unit.with_lsp(|lsp| {
				lsp.with_semantics(end_cursor.source(), |sems, tokens| {
					sems.add(captures.get(1).unwrap().range(), tokens.list_bullet);
					if let Some(props) = captures.get(2).map(|m| m.range()) {
						sems.add(props.start - 1..props.start, tokens.list_prop_sep);
						sems.add_to_queue(props.end..props.end + 1, tokens.list_prop_sep);
					}
					if let Some(props) = captures.get(3).map(|m| m.start() - 1..m.end() + 1) {
						sems.add_to_queue(props, tokens.list_bullet_type);
					}
				})
			});

			// Properties
			let prop_source = escape_source(
				end_cursor.source().clone(),
				captures.get(2).map_or(0..0, |m| m.range()),
				"List Properties".into(),
				'\\',
				"]",
			);
			let mut properties = match self.properties.parse(
				"List",
				unit,
				Token::new(0..prop_source.content().len(), prop_source),
			) {
				Some(props) => props,
				None => return false,
			};

			let offset =
				match properties.get_opt(unit, "offset", |_, value| value.value.parse::<usize>()) {
					Some(offset) => offset,
					_ => return false,
				};

			// Depth
			let depth = {
				let empty = vec![];
				let previous = list
					.entries
					.last()
					.map(|ent| &ent.markers)
					.unwrap_or(&empty);
				parse_depth(
					captures.get(1).unwrap().as_str(),
					offset.unwrap_or(1),
					previous,
				)
			};

			// Parse bullet
			let bullet = if let Some((bullet_range, bullet_content)) =
				captures.get(3).map(|m| (m.range(), m.as_str()))
			{
				let data = match bullet_content {
					"" | " " => BulletMarker::Checkbox(CheckboxState::Unchecked),
					"-" => BulletMarker::Checkbox(CheckboxState::Partial),
					"x" | "X" => BulletMarker::Checkbox(CheckboxState::Checked),
					_ => {
						report_err!(
							unit,
							end_cursor.source().clone(),
							"Unknown list bullet type".into(),
							span(
								bullet_range,
								format!(
									"Unknown bullet type: `{}`",
									bullet_content.fg(unit.colors().highlight),
								)
							)
						);
						return false;
					}
				};

				// Add conceal
				/*
				if let Some(conceals) =
					Conceals::from_source(cursor.source.clone(), &state.shared.lsp)
				{
					match data {
						CustomListData::Checkbox(checkbox_state) => conceals.add(
							custom_data.start - 1..custom_data.end + 1,
							ConcealTarget::Token {
								token: "checkbox".into(),
								params: json!({
									"state": checkbox_state,
								}),
							},
						),
					}
				}
				*/
				data
			} else {
				BulletMarker::Bullet
			};

			/*
			if let Some(conceals) =
				Conceals::from_source(cursor.source.clone(), &state.shared.lsp)
			{
				let mut i = captures.get(1).unwrap().start();
				for (depth, (numbered, _)) in depth.iter().enumerate() {
					conceals.add(
						i..i + 1,
						lsp::conceal::ConcealTarget::Token {
							token: "bullet".into(),
							params: json!({
								"depth": depth,
								"numbered": *numbered,
							}),
						},
					);
					i += 1;
				}
			}

			// Hints
			if let Some(hints) = Hints::from_source(cursor.source.clone(), &state.shared.lsp) {
				let mut label = String::new();
				for (_, id) in &depth {
					if !label.is_empty() {
						label.push('.');
					}
					label.push_str(id.to_string().as_str());
				}
				hints.add(captures.get(1).unwrap().end(), label);
			}
			*/

			// Content
			let entry_start = captures.get(4).unwrap().start();
			let mut entry_content = captures.get(4).unwrap().as_str().to_string();
			while let Some(captures) = self.continue_re.captures_at(content, end_cursor.pos()) {
				// Break if next element is another entry
				if captures.get(0).unwrap().start() != end_cursor.pos()
					|| captures
						.get(1)
						.unwrap()
						.as_str()
						.find(['*', '-'])
						.map(|delim| {
							captures.get(1).unwrap().as_str()[0..delim]
								.chars()
								.all(|c| c.is_whitespace())
						}) == Some(true)
				{
					break;
				}
				// Advance cursor
				end_cursor = end_cursor.at(captures.get(0).unwrap().end());

				entry_content += "\n";
				entry_content += captures.get(1).unwrap().as_str();
			}

			// Parse entry content
			let token = Token::new(entry_start..end_cursor.pos(), end_cursor.source().clone());
			let entry_src = Arc::new(VirtualSource::new(
				token.clone(),
				PathBuf::from("List Entry"),
				entry_content,
			));
			let parsed_content = match parse_paragraph(unit, entry_src) {
				Err(err) => {
					report_warn!(
						unit,
						token.source(),
						"Unable to parse List Entry".into(),
						span(token.range.clone(), err.into())
					);
					// Return an empty paragraph
					return false;
				}
				Ok(paragraph) => paragraph,
			};

			list.add_entry(ListEntry {
				location: Token::new(entry_start..end_cursor.pos(), end_cursor.source()),
				bullet,
				content: parsed_content,
				markers: depth,
			});
			true
		};

		while parse_entry() {}
		list.location.range = cursor.pos()..end_cursor.pos();
		unit.add_content(list);
		end_cursor
	}

	fn register_bindings(&self) {
		add_documented_function_values!(
			"list.Entry",
			|lua: &mlua::Lua, args: mlua::MultiValue| {
				let (bullet, content, markers) = convert_lua_args!(lua, args, (BulletMarker, "bullet"), (ScopeWrapper, "content", userdata), (Vec<ListMarker>, "markers"));
				Ok(Kernel::with_context(lua, |ctx| ListEntry {
					location: ctx.location.clone(),
					bullet,
					content: content.0.clone(),
					markers,
				}))
			},
			"Creates a new list entry",
			vec![
				"bullet BulletMarker Type of list bullet for this entry",
				"content Scope Content of this entry",
				"markers ListMarker[] Markers to this entry"
			],
			"ListEntry"
		);
		add_documented_function_values!(
			"list.List",
			|lua: &mlua::Lua, args: mlua::MultiValue| {
				let entries = convert_lua_args!(lua, args, (ListEntry, "entries", vuserdata));
				let contained = entries
					.iter()
					.map(|ent| ent.content.clone())
					.collect::<Vec<_>>();
				Ok(Kernel::with_context(lua, |ctx| ElemWrapper (Arc::new(List {
						location: ctx.location.clone(),
						contained,
						entries: entries.to_owned(),
					}),
				)))
			},
			"Creates a new list entry",
			vec!["entries ListEntry[] Entries for the list"],
			"List"
		);
	}

	fn completion(
		&self,
	) -> Option<Box<dyn lsp::completion::CompletionProvider + 'static + Send + Sync>> {
		Some(Box::new(ListCompletion {}))
	}
}
