use super::parser::Parser;
use super::source::{Cursor, Source, Token};
use ariadne::Report;
use mlua::{Function, Lua};
use crate::document::document::Document;

use std::any::Any;
use std::ops::Range;
use std::rc::Rc;

pub trait Rule {
	/// Returns rule's name
	fn name(&self) -> &'static str;
	/// Finds the next match starting from [`cursor`]
	fn next_match(&self, cursor: &Cursor) -> Option<(usize, Box<dyn Any>)>;
	/// Callback when rule matches
	fn on_match(&self, parser: &dyn Parser, document: &Document, cursor: Cursor, match_data: Option<Box<dyn Any>>) -> (Cursor, Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>);
	/// Export bindings to lua
	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Vec<(String, Function<'lua>)>;
}

impl core::fmt::Debug for dyn Rule
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Rule{{{}}}", self.name())
    }
}

/*
pub trait RegexRule: Rule
{
	fn name(&self) -> &'static str;

	/// Returns the rule's regex
	fn regex(&self) -> &regex::Regex;
	/// Callback on regex rule match
	fn on_regex_match<'a>(&self, parser: &Parser, document: &Document, token: Token<'a>, matches: regex::Captures) -> Vec<Report<'a, (String, Range<usize>)>>;
}

impl<T: RegexRule> Rule for T {
    fn name(&self) -> &'static str { RegexRule::name(self) }

	/// Finds the next match starting from [`cursor`]
	fn next_match<'a>(&self, cursor: &'a Cursor) -> Option<usize>
	{
		let re = self.regex();

		let content = cursor.file.content.as_ref().unwrap();
		match re.find_at(content.as_str(), cursor.pos)
		{
			Some(m) => Some(m.start()),
			None => None,
		}
	}

	fn on_match<'a>(&self, parser: &Parser, document: &Document, cursor: Cursor<'a>) -> (Cursor<'a>, Vec<Report<'a, (String, Range<usize>)>>)
	{
		let content = cursor.file.content.as_ref().unwrap();
		let matches = self.regex().captures_at(content.as_str(), cursor.pos).unwrap();
		let token = Token::new(cursor.pos, matches.get(0).unwrap().len(), cursor.file);

		let token_end = token.end();
		(cursor.at(token_end), self.on_regex_match(parser, document, token, matches))
	}
}
*/

pub trait RegexRule
{
	fn name(&self) -> &'static str;

	/// Returns the rule's regexes
	fn regexes(&self) -> &[regex::Regex];

	/// Callback on regex rule match
	fn on_regex_match(&self, index: usize, parser: &dyn Parser, document: &Document, token: Token, matches: regex::Captures) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>;

	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Vec<(String, Function<'lua>)>;
}

impl<T: RegexRule> Rule for T {
	fn name(&self) -> &'static str { RegexRule::name(self) }

	/// Finds the next match starting from [`cursor`]
	fn next_match(&self, cursor: &Cursor)
		-> Option<(usize, Box<dyn Any>)> {
		let content = cursor.source.content();
        let mut found: Option<(usize, usize)> = None;
        self.regexes().iter().enumerate().for_each(|(id, re)| {
            if let Some(m) = re.find_at(content.as_str(), cursor.pos)
            {
                found = found
					.and_then(|(f_pos, f_id)|
						if f_pos > m.start() { Some((m.start(), id)) } else { Some((f_pos, f_id)) } )
					.or(Some((m.start(), id)));
            }
        });

        return found.map(|(pos, id)|
			(pos, Box::new(id) as Box<dyn Any>));
	}

	fn on_match(&self, parser: &dyn Parser, document: &Document, cursor: Cursor, match_data: Option<Box<dyn Any>>)
		-> (Cursor, Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>) {
		let content = cursor.source.content();
		let index = unsafe { match_data.unwrap_unchecked().downcast::<usize>().unwrap_unchecked() };
		let re = &self.regexes()[*index];

		let captures = re.captures_at(content.as_str(), cursor.pos).unwrap();
		let token = Token::new(captures.get(0).unwrap().range(), cursor.source.clone());

		let token_end = token.end();
		return (cursor.at(token_end), self.on_regex_match(*index, parser, document, token, captures));
	}

	fn lua_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> { self.lua_bindings(lua) }
}
