use std::sync::Arc;

use nml_source::Token;

use crate::Options;
use crate::RuleId;

/// A node of the parse tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
	/// Text that no rule matched.
	Text(Token),
	/// `\` and an ASCII punctuation character, which stands for that character.
	Escape(Token),
	/// `::` and the rest of its line, at the start of a line or after a blank.
	Comment(Token),
	/// Inline content between blank lines that no block rule matched.
	Paragraph { token: Token, children: Vec<Node> },
	/// A match of a rule.
	Match(RuleMatch),
}

impl Node {
	pub fn token(&self) -> &Token {
		match self {
			Node::Text(token) | Node::Escape(token) | Node::Comment(token) => token,
			Node::Paragraph { token, .. } => token,
			Node::Match(m) => &m.token,
		}
	}
}

/// A match of a rule: the rule's callback builds document nodes from its captures.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleMatch {
	pub rule: RuleId,
	pub token: Token,
	/// Captures in the order they matched. A capture inside a `Repeat` appears once per
	/// repetition.
	pub captures: Vec<Capture>,
	/// False if the rule failed: the match then ends where it failed, and an error was
	/// reported.
	pub complete: bool,
}

impl RuleMatch {
	/// The first capture with this name.
	pub fn capture(&self, name: &str) -> Option<&Capture> {
		self.captures.iter().find(|capture| &*capture.name == name)
	}

	/// Every capture with this name.
	pub fn captures<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Capture> {
		self.captures
			.iter()
			.filter(move |capture| &*capture.name == name)
	}
}

/// A named part of a match.
#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
	pub name: Arc<str>,
	/// The semantic token class declared with the capture.
	pub class: Option<Arc<str>>,
	pub token: Token,
	pub value: Value,
}

/// What a capture holds, beyond its token.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
	/// The captured text is the value.
	Span,
	Number(f64),
	/// Parsed content.
	Nodes(Vec<Node>),
	Options(Options),
}
