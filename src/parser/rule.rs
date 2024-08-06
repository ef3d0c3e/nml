use super::layout::LayoutHolder;
use super::parser::ParserState;
use super::source::Cursor;
use super::source::Source;
use super::source::Token;
use super::style::StyleHolder;
use crate::document::document::Document;
use ariadne::Report;
use downcast_rs::impl_downcast;
use downcast_rs::Downcast;
use mlua::Function;
use mlua::Lua;

use std::any::Any;
use std::ops::Range;
use std::rc::Rc;

pub trait Rule: Downcast {
	/// Returns rule's name
	fn name(&self) -> &'static str;
	/// Finds the next match starting from [`cursor`]
	fn next_match(&self, state: &ParserState, cursor: &Cursor) -> Option<(usize, Box<dyn Any>)>;
	/// Callback when rule matches
	fn on_match<'a>(
		&self,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
		cursor: Cursor,
		match_data: Box<dyn Any>,
	) -> (Cursor, Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>);

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
	fn name(&self) -> &'static str;

	/// Returns the rule's regexes
	fn regexes(&self) -> &[regex::Regex];

	/// Callback on regex rule match
	fn on_regex_match<'a>(
		&self,
		index: usize,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
		token: Token,
		matches: regex::Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>;

	fn register_bindings<'lua>(&self, _lua: &'lua Lua) -> Vec<(String, Function<'lua>)> { vec![] }
	fn register_styles(&self, _holder: &mut StyleHolder) {}
	fn register_layouts(&self, _holder: &mut LayoutHolder) {}
}

impl<T: RegexRule + 'static> Rule for T {
	fn name(&self) -> &'static str { RegexRule::name(self) }

	/// Finds the next match starting from [`cursor`]
	fn next_match(&self, _state: &ParserState, cursor: &Cursor) -> Option<(usize, Box<dyn Any>)> {
		let content = cursor.source.content();
		let mut found: Option<(usize, usize)> = None;
		self.regexes().iter().enumerate().for_each(|(id, re)| {
			if let Some(m) = re.find_at(content.as_str(), cursor.pos) {
				found = found
					.and_then(|(f_pos, f_id)| {
						if f_pos > m.start() {
							Some((m.start(), id))
						} else {
							Some((f_pos, f_id))
						}
					})
					.or(Some((m.start(), id)));
			}
		});

		return found.map(|(pos, id)| (pos, Box::new(id) as Box<dyn Any>));
	}

	fn on_match<'a>(
		&self,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
		cursor: Cursor,
		match_data: Box<dyn Any>,
	) -> (Cursor, Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>) {
		let content = cursor.source.content();
		let index = match_data.downcast::<usize>().unwrap();
		let re = &self.regexes()[*index];

		let captures = re.captures_at(content.as_str(), cursor.pos).unwrap();
		let token = Token::new(captures.get(0).unwrap().range(), cursor.source.clone());

		let token_end = token.end();
		return (
			cursor.at(token_end),
			self.on_regex_match(*index, state, document, token, captures),
		);
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		self.register_bindings(lua)
	}

	fn register_styles(&self, holder: &mut StyleHolder) { self.register_styles(holder); }

	fn register_layouts(&self, holder: &mut LayoutHolder) { self.register_layouts(holder); }
}
