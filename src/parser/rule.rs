use super::source::Cursor;
use super::source::Token;
use super::state::CustomStates;
use super::state::ParseMode;
use crate::lsp::completion::CompletionProvider;
use crate::lua::kernel::Kernel;
use crate::unit::translation::TranslationUnit;
use downcast_rs::impl_downcast;
use downcast_rs::Downcast;

use std::any::Any;

macro_rules! create_registry {
	($($construct:expr);+ $(;)?) => {{
		let mut vec = Vec::new();
		$(
			let boxed = Box::new($construct) as Box<dyn Rule + Send + Sync>;
			vec.push(boxed);
		)+
		vec
	}};
}

/// Gets the list of all rules exported with the [`auto_registry`] proc macro.
/// Rules are sorted according to topological order using the [`Rule::previous`] method.
//#[auto_registry::generate_registry(registry = "rules", target = make_rules, return_type = Vec<Box<dyn Rule + Send + Sync>>, maker = create_registry)]
#[auto_registry::generate_registry(registry = "rules", collector = create_registry, output = get_rules)]

pub fn get_rule_registry() -> Vec<Box<dyn Rule + Send + Sync>> {
	let mut vec = get_rules!();
	vec.sort_by_key(|rule| rule.target());
	vec
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum RuleTarget {
	/// Meta characters target, e.g newlines
	Meta,
	/// Command statements
	Command,
	/// Block statements
	Block,
	/// Inline elements, e.g style
	Inline,
}

pub trait Rule: Downcast {
	/// Returns the name of the rule
	fn name(&self) -> &'static str;

	/// Rule ordering
	fn target(&self) -> RuleTarget;

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
		unit: &TranslationUnit,
		mode: &ParseMode,
		states: &mut CustomStates,
		cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any + Send + Sync>)>;

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
		unit: &mut TranslationUnit,
		cursor: &Cursor,
		match_data: Box<dyn Any + Send + Sync>,
	) -> Cursor;

	/// Registers lua bindings for this rule on the given kernel
	#[allow(unused_variables)]
	fn register_bindings(&self) {}

	/// Creates the completion provided associated with the rule
	fn completion(&self) -> Option<Box<dyn CompletionProvider + 'static + Send + Sync>> {
		None
	}
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

	/// Rule ordering
	fn target(&self) -> RuleTarget;

	/// Returns the rule's regexes
	fn regexes(&self) -> &[regex::Regex];

	/// Checks whether the rule should be enabled for a given [`ParseMode`].
	///
	/// # Parameters
	///
	/// `index` represents the index of the regex (given by [`Self::regexes`]) that is checked
	/// against.
	fn enabled(
		&self,
		unit: &TranslationUnit,
		mode: &ParseMode,
		states: &mut CustomStates,
		index: usize,
	) -> bool;

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
		unit: &mut TranslationUnit,
		token: Token,
		captures: regex::Captures,
	);

	#[allow(unused_variables)]
	fn register_bindings(&self) {}

	fn completion(&self) -> Option<Box<dyn CompletionProvider + 'static + Send + Sync>> {
		None
	}
}

impl<T: RegexRule + 'static> Rule for T {
	fn name(&self) -> &'static str {
		RegexRule::name(self)
	}

	fn target(&self) -> RuleTarget {
		RegexRule::target(self)
	}

	/// Finds the next match starting from [`Cursor`]
	fn next_match(
		&self,
		unit: &TranslationUnit,
		mode: &ParseMode,
		states: &mut CustomStates,
		cursor: &Cursor,
	) -> Option<(usize, Box<dyn Any + Send + Sync>)> {
		let source = cursor.source();
		let content = source.content();

		let mut found: Option<(usize, usize)> = None;
		self.regexes().iter().enumerate().for_each(|(id, re)| {
			if !RegexRule::enabled(self, unit, mode, states, id) {
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

		found.map(|(pos, id)| (pos, Box::new(id) as Box<dyn Any + Send + Sync>))
	}

	fn on_match<'u>(
		&self,
		unit: &mut TranslationUnit,
		cursor: &Cursor,
		match_data: Box<dyn Any + Send + Sync>,
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

	fn register_bindings(&self) {
		self.register_bindings()
	}

	fn completion(&self) -> Option<Box<dyn CompletionProvider + 'static + Send + Sync>> {
		self.completion()
	}
}

#[cfg(test)]
mod tests {

	#[test]
	fn registry() {
		/*
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
		*/
	}
}
