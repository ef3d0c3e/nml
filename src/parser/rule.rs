use super::reports::Report;
use super::source::Cursor;
use super::source::Token;
use super::state::ParseMode;
use super::state::ParserState;
use super::translation::TranslationUnit;
use crate::document::document::Document;
use crate::lua::kernel::Kernel;
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
	/// Returns the name of the rule
	fn name(&self) -> &'static str;

	/// Returns the name of the rule that should preceed this one in terms of priority
	fn previous(&self) -> Option<&'static str>;

	/// Finds the next match starting from `cursor`
	///
	/// # Return
	///
	/// This method returns the position of the next match (if any) as well as data that needs to
	/// be passed to [`Self::on_match`] when the rules is chosen. It is the job of the parser to
	/// keep track of this temporary data.
	///
	/// # Parameters
	///
	/// `mode` Specifies the current parser mode. Some elements should behave differently for different
	/// modes. For instance mode `paragraph_only` makes the rule for `Section`s to be ignored.
	fn next_match(
		&self,
		mode: &ParseMode,
		cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any>)>;

	/// Method called when the rule is chosen by the parser.
	///
	/// # Return
	///
	/// This function must return the cursor position after processing the match, as well as a list
	/// of reports generated during processing. In case of error, the parser may continue parsing,
	/// therefore it is required that this method advances the cursor to prevent infinite loops.
	///
	/// # Parameters
	///
	/// `match_data` is the temporary returned by [`Self::on_match`].
	fn on_match<'u>(
		&self,
		unit: &mut TranslationUnit<'u>,
		cursor: &Cursor,
		match_data: Box<dyn Any>,
	) -> Cursor;

	/// Registers lua bindings for this rule on the given kernel
	#[allow(unused_variables)]
	fn register_bindings<'lua>(&self, kernel: &'lua Kernel, table: mlua::Table) { }
}
impl_downcast!(Rule);

impl core::fmt::Debug for dyn Rule {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Rule{{{}}}", self.name())
	}
}

pub trait RegexRule {
	/// Returns the name of the rule
	fn name(&self) -> &'static str;

	/// Returns the name of the rule that should preceed this one in terms of priority
	fn previous(&self) -> Option<&'static str>;

	/// Returns the rule's regexes
	fn regexes(&self) -> &[regex::Regex];

	/// Checks whether the rule should be enabled for a given [`ParseMode`].
	///
	/// # Parameters
	///
	/// `index` represents the index of the regex (given by [`Self::regexes`]) that is checked
	/// against.
	fn enabled(&self, mode: &ParseMode, index: usize) -> bool;

	/// Method called when the rule is chosen by the parser
	///
	/// # Parameters
	///  * `index` Index of the matching rule in the table returned by [`Self::regexes`]
	///  * `unit` The translation unit
	///  * `token` Token formed by this match
	///  * `captures` Regex captures data
	fn on_regex_match<'u>(
		&self,
		index: usize,
		unit: &mut TranslationUnit<'u>,
		token: Token,
		captures: regex::Captures,
	);

	#[allow(unused_variables)]
	fn register_bindings<'lua>(&self, kernel: &'lua Kernel, table: mlua::Table) { }
}

impl<T: RegexRule + 'static> Rule for T {
	fn name(&self) -> &'static str { RegexRule::name(self) }

	fn previous(&self) -> Option<&'static str> { RegexRule::previous(self) }

	/// Finds the next match starting from [`Cursor`]
	fn next_match(
		&self,
		mode: &ParseMode,
		cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any>)> {
		let source = cursor.source();
		let content = source.content();

		let mut found: Option<(usize, usize)> = None;
		self.regexes().iter().enumerate().for_each(|(id, re)| {
			if !RegexRule::enabled(self, mode, id) {
				return;
			}
			if let Some(m) = re.find_at(content.as_str(), cursor.pos()) {
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

	fn on_match<'u>(
		&self,
		unit: &mut TranslationUnit<'u>,
		cursor: &Cursor,
		match_data: Box<dyn Any>,
	) -> Cursor {
		let source = cursor.source();
		let content = source.content();

		let index = match_data.downcast::<usize>().unwrap();
		let re = &self.regexes()[*index];

		let captures = re.captures_at(content.as_str(), cursor.pos()).unwrap();
		let token = Token::new(captures.get(0).unwrap().range(), cursor.source());

		let token_end = token.end();
		self.on_regex_match(*index, unit, token, captures);
		cursor.at(token_end)
	}

	fn register_bindings<'lua>(&self, kernel: &'lua Kernel, table: mlua::Table) {
		self.register_bindings(kernel, table)
	}
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
				"Block",
				"Code",
				"Tex",
				"Graphviz",
				"Media",
				"Layout",
				"Toc",
				"Table",
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
