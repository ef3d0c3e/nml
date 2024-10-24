use super::layout::LayoutHolder;
use super::parser::ParseMode;
use super::parser::ParserState;
use super::reports::Report;
use super::source::Cursor;
use super::source::Token;
use super::style::StyleHolder;
use crate::document::document::Document;
use downcast_rs::impl_downcast;
use downcast_rs::Downcast;
use mlua::Function;
use mlua::Lua;

use std::any::Any;
use std::collections::HashMap;

macro_rules! create_registry {
	( $($construct:expr),+ $(,)? ) => {{
		let mut map = HashMap::new();
		$(
			let boxed = Box::new($construct) as Box<dyn Rule>;
			map.insert(boxed.name(), boxed);
		)+
		map
	}};
}

/// Gets the list of all rules exported with the [`auto_registry`] proc macro.
/// Rules are sorted according to topological order using the [`Rule::previous`] method.
#[auto_registry::generate_registry(registry = "rules", target = make_rules, return_type = HashMap<&'static str, Box<dyn Rule>>, maker = create_registry)]
pub fn get_rule_registry() -> Vec<Box<dyn Rule>> {
	fn cmp(
		map: &HashMap<&'static str, Box<dyn Rule>>,
		lname: &'static str,
		rname: &'static str,
	) -> std::cmp::Ordering {
		let l = map.get(lname).unwrap();
		let r = map.get(rname).unwrap();
		if l.previous() == Some(r.name()) {
			std::cmp::Ordering::Greater
		} else if r.previous() == Some(l.name()) {
			std::cmp::Ordering::Less
		} else if l.previous().is_some() && r.previous().is_none() {
			std::cmp::Ordering::Greater
		} else if r.previous().is_some() && l.previous().is_none() {
			std::cmp::Ordering::Less
		} else if let (Some(pl), Some(pr)) = (l.previous(), r.previous()) {
			cmp(map, pl, pr)
		} else {
			std::cmp::Ordering::Equal
		}
	}
	let mut map = make_rules();
	let mut sorted_keys = map.keys().copied().collect::<Vec<_>>();
	sorted_keys.sort_by(|l, r| cmp(&map, l, r));

	let mut owned = Vec::with_capacity(sorted_keys.len());
	for key in sorted_keys {
		let rule = map.remove(key).unwrap();
		owned.push(rule);
	}

	owned
}

pub trait Rule: Downcast {
	/// The rule name
	fn name(&self) -> &'static str;

	/// The name of the rule that should come before this one
	fn previous(&self) -> Option<&'static str>;

	/// Finds the next match starting from [`cursor`]
	fn next_match(
		&self,
		mode: &ParseMode,
		state: &ParserState,
		cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any>)>;

	/// Callback when rule matches
	fn on_match<'a>(
		&self,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
		cursor: Cursor,
		match_data: Box<dyn Any>,
	) -> (Cursor, Vec<Report>);

	/// Registers lua bindings
	fn register_bindings<'lua>(&self, _lua: &'lua Lua) -> Vec<(String, Function<'lua>)> { vec![] }

	/// Registers default styles
	fn register_styles(&self, _holder: &mut StyleHolder) {}

	/// Registers default layouts
	fn register_layouts(&self, _holder: &mut LayoutHolder) {}
}
impl_downcast!(Rule);

impl core::fmt::Debug for dyn Rule {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Rule{{{}}}", self.name())
	}
}

pub trait RegexRule {
	/// The rule name
	fn name(&self) -> &'static str;

	/// The name of the rule that should come before this one
	fn previous(&self) -> Option<&'static str>;

	/// Returns the rule's regexes
	fn regexes(&self) -> &[regex::Regex];

	/// Wheter parsing for the rule is enabled
	fn enabled(&self, mode: &ParseMode, index: usize) -> bool;

	/// Callback on regex rule match
	fn on_regex_match<'a>(
		&self,
		index: usize,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
		token: Token,
		matches: regex::Captures,
	) -> Vec<Report>;

	fn register_bindings<'lua>(&self, _lua: &'lua Lua) -> Vec<(String, Function<'lua>)> { vec![] }
	fn register_styles(&self, _holder: &mut StyleHolder) {}
	fn register_layouts(&self, _holder: &mut LayoutHolder) {}
}

impl<T: RegexRule + 'static> Rule for T {
	fn name(&self) -> &'static str { RegexRule::name(self) }

	fn previous(&self) -> Option<&'static str> { RegexRule::previous(self) }

	/// Finds the next match starting from [`cursor`]
	fn next_match(
		&self,
		mode: &ParseMode,
		_state: &ParserState,
		cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any>)> {
		let content = cursor.source.content();
		let mut found: Option<(usize, usize)> = None;
		self.regexes().iter().enumerate().for_each(|(id, re)| {
			if !RegexRule::enabled(self, mode, id) {
				return;
			}
			if let Some(m) = re.find_at(content.as_str(), cursor.pos) {
				found = found
					.map(|(f_pos, f_id)| {
						if f_pos > m.start() {
							(m.start(), id)
						} else {
							(f_pos, f_id)
						}
					})
					.or(Some((m.start(), id)));
			}
		});

		found.map(|(pos, id)| (pos, Box::new(id) as Box<dyn Any>))
	}

	fn on_match<'a>(
		&self,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
		cursor: Cursor,
		match_data: Box<dyn Any>,
	) -> (Cursor, Vec<Report>) {
		let content = cursor.source.content();
		let index = match_data.downcast::<usize>().unwrap();
		let re = &self.regexes()[*index];

		let captures = re.captures_at(content.as_str(), cursor.pos).unwrap();
		let token = Token::new(captures.get(0).unwrap().range(), cursor.source.clone());

		let token_end = token.end();
		(
			cursor.at(token_end),
			self.on_regex_match(*index, state, document, token, captures),
		)
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		self.register_bindings(lua)
	}

	fn register_styles(&self, holder: &mut StyleHolder) { self.register_styles(holder); }

	fn register_layouts(&self, holder: &mut LayoutHolder) { self.register_layouts(holder); }
}

#[cfg(test)]
mod tests {

	use super::*;

	#[test]
	fn registry() {
		let rules = get_rule_registry();
		let names: Vec<&'static str> = rules.iter().map(|rule| rule.name()).collect();

		assert_eq!(
			names,
			vec![
				"Comment",
				"Paragraph",
				"Import",
				"Script",
				"Element Style",
				"Variable",
				"Variable Substitution",
				"Raw",
				"List",
				"Blockquote",
				"Code",
				"Tex",
				"Graphviz",
				"Media",
				"Layout",
				"Style",
				"Custom Style",
				"Section",
				"Link",
				"Text",
				"Reference",
			]
		);
	}
}
