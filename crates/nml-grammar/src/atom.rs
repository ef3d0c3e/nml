use std::sync::Arc;

use crate::OptionSchema;

/// One element of a rule.
///
/// A rule is a sequence of atoms, matched in order without backtracking: `Or` and `Repeat`
/// commit to a branch as soon as its first atom matches.
#[derive(Debug, Clone, PartialEq)]
pub enum Atom {
	/// Exactly this text.
	Literal(String),
	/// The start of a line (zero width).
	LineStart,
	/// A line break, or the end of the input (zero width).
	LineEnd,
	/// The first alternative whose first atom matches.
	Or(Vec<Vec<Atom>>),
	/// The atoms, repeated while their first atom matches, between `min` and `max` times.
	Repeat {
		atoms: Vec<Atom>,
		min: usize,
		max: Option<usize>,
	},
	/// A decimal number, optionally signed and with a fractional part.
	Number { signed: bool, decimal: bool },
	/// An identifier: a letter or `_`, then letters, digits, `_` and `-`.
	Word,
	/// Verbatim text up to the delimiter that follows.
	Text,
	/// Verbatim text up to the end of the line, without the line break.
	RestOfLine,
	/// Parsed inline content up to the delimiter that follows, within one paragraph.
	InlineContent,
	/// Parsed blocks up to the line-anchored delimiter that follows.
	Content,
	/// Names the part of the match covered by `atom`, for the rule's callback. `class` is the
	/// capture's semantic token class for the LSP (`keyword`, `string`, …).
	Capture {
		name: String,
		class: Option<String>,
		atom: Box<Atom>,
	},
	/// A run of at least `min` times the same character, e.g. a code fence.
	Run { char: char, min: usize },
	/// Exactly the text of an earlier capture. For a captured `Run`, a run of exactly the
	/// same length: inside a ```` ``` ```` fence, ```` ```` ```` does not close it.
	Same(String),
	/// An option list, `[key=value, …]`, on one line, checked against a schema.
	Options(Arc<OptionSchema>),
	/// The rest of the line and the following lines that carry the prefix, with the prefix
	/// removed, parsed as blocks. It must be the last atom of a block rule.
	Container(Prefix),
}

/// The line prefix of a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prefix {
	/// Lines that start with this text. A line that is the prefix without its trailing
	/// blanks (`>` for `> `) is an empty line of the container.
	Literal(String),
	/// Lines indented by the width of the first line up to the container (a list item's
	/// marker). Blank lines are part of the container when an indented line follows them.
	Indent,
}

impl Atom {
	pub fn literal(text: impl Into<String>) -> Self {
		Atom::Literal(text.into())
	}

	pub fn capture(name: impl Into<String>, atom: Atom) -> Self {
		Atom::Capture {
			name: name.into(),
			class: None,
			atom: Box::new(atom),
		}
	}

	/// Sets the semantic token class of a capture.
	///
	/// # Panics
	///
	/// If the atom is not a capture.
	pub fn with_class(mut self, class: impl Into<String>) -> Self {
		match &mut self {
			Atom::Capture { class: slot, .. } => *slot = Some(class.into()),
			_ => panic!("only a capture has a semantic token class"),
		}
		self
	}

	pub fn run(char: char, min: usize) -> Self {
		Atom::Run { char, min }
	}

	pub fn same(name: impl Into<String>) -> Self {
		Atom::Same(name.into())
	}

	pub fn or(alternatives: impl IntoIterator<Item = Vec<Atom>>) -> Self {
		Atom::Or(alternatives.into_iter().collect())
	}

	pub fn repeat(atoms: Vec<Atom>, min: usize, max: Option<usize>) -> Self {
		Atom::Repeat { atoms, min, max }
	}

	/// The atoms, zero or one time.
	pub fn optional(atoms: Vec<Atom>) -> Self {
		Atom::repeat(atoms, 0, Some(1))
	}

	/// A short description, for errors.
	pub fn describe(&self) -> &'static str {
		match self {
			Atom::Literal(_) => "Literal",
			Atom::LineStart => "LineStart",
			Atom::LineEnd => "LineEnd",
			Atom::Or(_) => "Or",
			Atom::Repeat { .. } => "Repeat",
			Atom::Number { .. } => "Number",
			Atom::Word => "Word",
			Atom::Text => "Text",
			Atom::RestOfLine => "RestOfLine",
			Atom::InlineContent => "InlineContent",
			Atom::Content => "Content",
			Atom::Capture { .. } => "Capture",
			Atom::Run { .. } => "Run",
			Atom::Same(_) => "Same",
			Atom::Options(_) => "Options",
			Atom::Container(_) => "Container",
		}
	}

	/// Whether the atom can tell from the current position alone if it matches, which is what
	/// `Or` and `Repeat` need to commit to a branch.
	pub(crate) fn decidable(&self) -> bool {
		match self {
			Atom::Literal(_)
			| Atom::LineStart
			| Atom::LineEnd
			| Atom::Number { .. }
			| Atom::Word
			| Atom::Run { .. }
			| Atom::Same(_)
			| Atom::Options(_) => true,
			Atom::Capture { atom, .. } => atom.decidable(),
			Atom::Or(alternatives) => alternatives
				.iter()
				.all(|atoms| atoms.first().is_some_and(Atom::decidable)),
			Atom::Repeat { atoms, min, .. } => {
				*min > 0 && atoms.first().is_some_and(Atom::decidable)
			}
			Atom::Text
			| Atom::RestOfLine
			| Atom::InlineContent
			| Atom::Content
			| Atom::Container(_) => false,
		}
	}
}

/// A compiled atom: open-ended atoms know the delimiters that can end them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Op {
	Literal(String),
	LineStart,
	LineEnd,
	Or(Vec<Vec<Op>>),
	Repeat {
		ops: Vec<Op>,
		min: usize,
		max: Option<usize>,
	},
	Number {
		signed: bool,
		decimal: bool,
	},
	Word,
	Text(Vec<DelimiterPattern>),
	RestOfLine,
	InlineContent(Vec<DelimiterPattern>),
	Content(Vec<DelimiterPattern>),
	Capture {
		name: Arc<str>,
		class: Option<Arc<str>>,
		op: Box<Op>,
	},
	Run {
		char: char,
		min: usize,
	},
	Same {
		name: Arc<str>,
		/// The character of the captured run, if the capture is a `Run`.
		run: Option<char>,
	},
	Options(Arc<OptionSchema>),
	Container(Prefix),
}

impl Op {
	pub(crate) fn describe(&self) -> &'static str {
		match self {
			Op::Literal(_) => "Literal",
			Op::LineStart => "LineStart",
			Op::LineEnd => "LineEnd",
			Op::Or(_) => "Or",
			Op::Repeat { .. } => "Repeat",
			Op::Number { .. } => "Number",
			Op::Word => "Word",
			Op::Text(_) => "Text",
			Op::RestOfLine => "RestOfLine",
			Op::InlineContent(_) => "InlineContent",
			Op::Content(_) => "Content",
			Op::Capture { op, .. } => op.describe(),
			Op::Run { .. } => "Run",
			Op::Same { .. } => "Same",
			Op::Options(_) => "Options",
			Op::Container(_) => "Container",
		}
	}
}

/// What ends an open-ended atom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DelimiterPattern {
	/// Only counts at the start of a line.
	pub anchored: bool,
	pub kind: DelimiterKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DelimiterKind {
	Literal(String),
	LineEnd,
	/// The text of a capture, see [`Atom::Same`].
	Same {
		name: Arc<str>,
		run: Option<char>,
	},
}

/// The delimiters that can come first in the concatenation of `chain`, or `None` if
/// something else can.
pub(crate) fn first_delimiters(chain: &[&[Atom]], anchored: bool) -> Option<Vec<DelimiterPattern>> {
	let (head, tail) = chain.split_first()?;
	let Some((atom, rest)) = head.split_first() else {
		return first_delimiters(tail, anchored);
	};
	fn join<'a>(atoms: &[&'a [Atom]], rest: &'a [Atom], tail: &[&'a [Atom]]) -> Vec<&'a [Atom]> {
		let mut chain = atoms.to_vec();
		chain.push(rest);
		chain.extend_from_slice(tail);
		chain
	}
	let pattern = |kind| Some(vec![DelimiterPattern { anchored, kind }]);
	match atom {
		Atom::LineStart if !anchored => first_delimiters(&join(&[], rest, tail), true),
		Atom::Literal(text) => pattern(DelimiterKind::Literal(text.clone())),
		Atom::LineEnd if !anchored => pattern(DelimiterKind::LineEnd),
		Atom::Same(name) => pattern(DelimiterKind::Same {
			name: name.as_str().into(),
			run: None,
		}),
		Atom::Capture { atom, .. } => {
			first_delimiters(&join(&[std::slice::from_ref(atom)], rest, tail), anchored)
		}
		Atom::Or(alternatives) => {
			let mut patterns = Vec::new();
			for atoms in alternatives {
				patterns.extend(first_delimiters(&join(&[atoms], rest, tail), anchored)?);
			}
			Some(patterns)
		}
		Atom::Repeat { atoms, min, .. } => {
			let repeated = first_delimiters(&join(&[atoms], rest, tail), anchored)?;
			if *min > 0 {
				return Some(repeated);
			}
			// What follows an optional part comes first: it is the delimiter an error names.
			let mut patterns = first_delimiters(&join(&[], rest, tail), anchored)?;
			patterns.extend(repeated);
			Some(patterns)
		}
		_ => None,
	}
}
