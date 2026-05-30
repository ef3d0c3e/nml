use std::collections::HashMap;
use std::fmt;

use crate::Atom;
use crate::atom::DelimiterKind;
use crate::atom::DelimiterPattern;
use crate::atom::Op;
use crate::atom::first_delimiters;
use crate::trie::Trie;

/// A rule of the grammar: a name and the atoms it matches.
///
/// A rule whose first atom is [`Atom::LineStart`] is a block rule; any other rule is inline.
/// After the optional `LineStart`, a rule starts with its opener: a [`Atom::Literal`],
/// possibly captured.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
	pub name: String,
	pub atoms: Vec<Atom>,
	/// The name of a rule this one replaces. Without it, a rule with the same opener as
	/// another is an error.
	pub replaces: Option<String>,
}

impl Rule {
	pub fn new(name: impl Into<String>, atoms: Vec<Atom>) -> Self {
		Self {
			name: name.into(),
			atoms,
			replaces: None,
		}
	}

	pub fn replacing(mut self, name: impl Into<String>) -> Self {
		self.replaces = Some(name.into());
		self
	}

	pub fn table(&self) -> Table {
		match self.atoms.first() {
			Some(Atom::LineStart) => Table::Block,
			_ => Table::Inline,
		}
	}

	/// The opener: the text the rule is dispatched on. For a `Run`, its shortest run.
	pub fn opener(&self) -> Option<String> {
		let first = match self.table() {
			Table::Block => self.atoms.get(1),
			Table::Inline => self.atoms.first(),
		};
		let mut atom = first?;
		while let Atom::Capture { atom: inner, .. } = atom {
			atom = inner;
		}
		match atom {
			Atom::Literal(text) if !text.is_empty() => Some(text.clone()),
			Atom::Run { char, min } if *min > 0 => Some(char.to_string().repeat(*min)),
			_ => None,
		}
	}
}

/// The dispatch table a rule belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Table {
	/// Rules anchored at the start of a line.
	Block,
	/// Rules inside inline content.
	Inline,
}

/// Identifies a rule in its [`RuleSet`]. IDs stay valid when a rule is replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RuleId(pub(crate) usize);

/// A rule that cannot be added to a [`RuleSet`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleError {
	/// The rule being added.
	pub rule: String,
	pub kind: RuleErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleErrorKind {
	/// Another rule already has this name.
	DuplicateName,
	/// The rule does not start with a literal opener.
	NoOpener,
	/// Another rule in the same table has the same opener.
	ConflictingOpener { other: String, opener: String },
	/// `replaces` names a rule that does not exist.
	UnknownReplaced { name: String },
	/// An open-ended atom is not followed by a delimiter.
	NotTerminated { atom: &'static str },
	/// `Content` is followed by a delimiter that is not at the start of a line.
	ContentNotLineAnchored,
	/// A `Repeat` or `Or` branch does not start with an atom that decides whether it matches.
	UndecidableBranch { atom: &'static str },
	/// A `Repeat` with `max` below `min`, or a `max` of zero.
	InvalidRepeatBounds,
	/// An empty `Or` alternative or `Repeat` body.
	EmptySequence,
	/// `Same` refers to a capture that is not defined before it.
	UnknownCapture { name: String },
	/// A `Run` with a `min` of zero.
	EmptyRun,
	/// A `Container` that is not the last atom of a block rule.
	MisplacedContainer,
}

impl fmt::Display for RuleError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "rule `{}`: ", self.rule)?;
		match &self.kind {
			RuleErrorKind::DuplicateName => write!(f, "a rule with this name already exists"),
			RuleErrorKind::NoOpener => write!(f, "the rule must start with a literal opener"),
			RuleErrorKind::ConflictingOpener { other, opener } => write!(
				f,
				"opener `{opener}` is already used by rule `{other}` (use `replaces` to override it)"
			),
			RuleErrorKind::UnknownReplaced { name } => {
				write!(f, "replaces `{name}`, which does not exist")
			}
			RuleErrorKind::NotTerminated { atom } => {
				write!(f, "`{atom}` must be followed by a delimiter")
			}
			RuleErrorKind::ContentNotLineAnchored => write!(
				f,
				"`Content` must be followed by a delimiter at the start of a line"
			),
			RuleErrorKind::UndecidableBranch { atom } => write!(
				f,
				"a branch starts with `{atom}`, which cannot decide whether the branch matches"
			),
			RuleErrorKind::InvalidRepeatBounds => write!(f, "invalid `Repeat` bounds"),
			RuleErrorKind::EmptySequence => write!(f, "empty `Or` alternative or `Repeat` body"),
			RuleErrorKind::UnknownCapture { name } => {
				write!(
					f,
					"`Same` refers to `{name}`, which is not captured before it"
				)
			}
			RuleErrorKind::EmptyRun => write!(f, "a `Run` must have a `min` of at least 1"),
			RuleErrorKind::MisplacedContainer => {
				write!(f, "a `Container` must be the last atom of a block rule")
			}
		}
	}
}

impl std::error::Error for RuleError {}

/// The rules of a grammar, checked when they are added.
#[derive(Debug, Default, Clone)]
pub struct RuleSet {
	/// Indexed by [`RuleId`]; `None` once replaced.
	rules: Vec<Option<Entry>>,
	by_name: HashMap<String, RuleId>,
	block: Trie,
	inline: Trie,
}

#[derive(Debug, Clone)]
struct Entry {
	rule: Rule,
	ops: Vec<Op>,
}

impl RuleSet {
	pub fn new() -> Self {
		Self::default()
	}

	/// Checks and adds a rule.
	pub fn add(&mut self, rule: Rule) -> Result<RuleId, RuleError> {
		let error = |kind| RuleError {
			rule: rule.name.clone(),
			kind,
		};
		let replaced = match &rule.replaces {
			Some(name) => Some(
				*self
					.by_name
					.get(name)
					.ok_or_else(|| error(RuleErrorKind::UnknownReplaced { name: name.clone() }))?,
			),
			None => None,
		};
		if self
			.by_name
			.get(&rule.name)
			.is_some_and(|id| Some(*id) != replaced)
		{
			return Err(error(RuleErrorKind::DuplicateName));
		}
		let opener = rule
			.opener()
			.ok_or_else(|| error(RuleErrorKind::NoOpener))?;
		for (id, other) in self.iter() {
			if Some(id) != replaced
				&& other.table() == rule.table()
				&& other.opener().as_ref() == Some(&opener)
			{
				return Err(error(RuleErrorKind::ConflictingOpener {
					other: other.name.clone(),
					opener: opener.clone(),
				}));
			}
		}
		check_containers(&rule).map_err(error)?;
		let ops = compile(&rule.atoms, &[Vec::new()], &mut Vec::new()).map_err(error)?;

		if let Some(replaced) = replaced {
			let old = self.rules[replaced.0].take().unwrap();
			self.by_name.remove(&old.rule.name);
		}
		let id = RuleId(self.rules.len());
		self.by_name.insert(rule.name.clone(), id);
		self.rules.push(Some(Entry { rule, ops }));
		self.build_tries();
		Ok(id)
	}

	fn build_tries(&mut self) {
		let mut block = Trie::default();
		let mut inline = Trie::default();
		for (id, rule) in self.iter() {
			let trie = match rule.table() {
				Table::Block => &mut block,
				Table::Inline => &mut inline,
			};
			trie.insert(&rule.opener().unwrap(), id);
		}
		self.block = block;
		self.inline = inline;
	}

	/// The rule with the longest opener at the start of `text`, and the opener's length.
	///
	/// For the block table, `text` starts at the beginning of a line. There is no fallback: if
	/// the rule then fails to match, that is an error.
	pub fn dispatch(&self, table: Table, text: &str) -> Option<(RuleId, usize)> {
		match table {
			Table::Block => self.block.longest(text),
			Table::Inline => self.inline.longest(text),
		}
	}

	pub fn get(&self, id: RuleId) -> Option<&Rule> {
		Some(&self.rules.get(id.0)?.as_ref()?.rule)
	}

	pub(crate) fn ops(&self, id: RuleId) -> &[Op] {
		&self.rules[id.0].as_ref().unwrap().ops
	}

	pub fn id(&self, name: &str) -> Option<RuleId> {
		self.by_name.get(name).copied()
	}

	/// The active rules, in the order they were added.
	pub fn iter(&self) -> impl Iterator<Item = (RuleId, &Rule)> {
		self.rules
			.iter()
			.enumerate()
			.filter_map(|(index, entry)| Some((RuleId(index), &entry.as_ref()?.rule)))
	}
}

/// The captures defined so far in a rule, with the character of captured runs.
type Captured = Vec<(String, Option<char>)>;

/// Checks the atoms of a sequence and compiles them. `follows` lists what can come after the
/// sequence: one chain of atom sequences per possibility.
fn compile(
	atoms: &[Atom],
	follows: &[Vec<&[Atom]>],
	captured: &mut Captured,
) -> Result<Vec<Op>, RuleErrorKind> {
	let mut ops = Vec::with_capacity(atoms.len());
	for (index, atom) in atoms.iter().enumerate() {
		let rest = &atoms[index + 1..];
		let chains: Vec<Vec<&[Atom]>> = follows
			.iter()
			.map(|follow| {
				let mut chain = vec![rest];
				chain.extend_from_slice(follow);
				chain
			})
			.collect();
		let delimiters = |captured: &Captured| -> Result<Vec<DelimiterPattern>, RuleErrorKind> {
			let mut delimiters = Vec::new();
			for chain in &chains {
				for mut delimiter in
					first_delimiters(chain, false).ok_or(RuleErrorKind::NotTerminated {
						atom: atom.describe(),
					})? {
					if let DelimiterKind::Same { name, run } = &mut delimiter.kind {
						*run = same(captured, name)?;
					}
					if !delimiters.contains(&delimiter) {
						delimiters.push(delimiter);
					}
				}
			}
			Ok(delimiters)
		};
		ops.push(match atom {
			Atom::Literal(text) => Op::Literal(text.clone()),
			Atom::LineStart => Op::LineStart,
			Atom::LineEnd => Op::LineEnd,
			Atom::Number { signed, decimal } => Op::Number {
				signed: *signed,
				decimal: *decimal,
			},
			Atom::Word => Op::Word,
			Atom::RestOfLine => Op::RestOfLine,
			Atom::Text => Op::Text(delimiters(captured)?),
			Atom::InlineContent => Op::InlineContent(delimiters(captured)?),
			Atom::Content => {
				let delimiters = delimiters(captured)?;
				if delimiters.iter().any(|d| !d.anchored) {
					return Err(RuleErrorKind::ContentNotLineAnchored);
				}
				Op::Content(delimiters)
			}
			Atom::Capture { name, class, atom } => {
				let op = compile(std::slice::from_ref(atom), &chains, captured)?
					.pop()
					.unwrap();
				let run = match &op {
					Op::Run { char, .. } => Some(*char),
					_ => None,
				};
				captured.push((name.clone(), run));
				Op::Capture {
					name: name.as_str().into(),
					class: class.as_deref().map(Into::into),
					op: Box::new(op),
				}
			}
			Atom::Run { char, min } => {
				if *min == 0 {
					return Err(RuleErrorKind::EmptyRun);
				}
				Op::Run {
					char: *char,
					min: *min,
				}
			}
			Atom::Options(schema) => Op::Options(schema.clone()),
			Atom::Container(prefix) => Op::Container(prefix.clone()),
			Atom::Same(name) => Op::Same {
				name: name.as_str().into(),
				run: same(captured, name)?,
			},
			Atom::Or(alternatives) => {
				let mut compiled = Vec::new();
				for alternative in alternatives {
					check_branch(alternative)?;
					compiled.push(compile(alternative, &chains, captured)?);
				}
				Op::Or(compiled)
			}
			Atom::Repeat { atoms, min, max } => {
				if max.is_some_and(|max| max == 0 || max < *min) {
					return Err(RuleErrorKind::InvalidRepeatBounds);
				}
				check_branch(atoms)?;
				let mut inner = chains.clone();
				inner.push(vec![atoms]);
				Op::Repeat {
					ops: compile(atoms, &inner, captured)?,
					min: *min,
					max: *max,
				}
			}
		});
	}
	Ok(ops)
}

/// A container may only be the last atom of a block rule, possibly captured.
fn check_containers(rule: &Rule) -> Result<(), RuleErrorKind> {
	fn contains(atom: &Atom) -> bool {
		match atom {
			Atom::Container(_) => true,
			Atom::Capture { atom, .. } => contains(atom),
			Atom::Or(alternatives) => alternatives.iter().flatten().any(contains),
			Atom::Repeat { atoms, .. } => atoms.iter().any(contains),
			_ => false,
		}
	}
	let Some((last, rest)) = rule.atoms.split_last() else {
		return Ok(());
	};
	let mut last = last;
	while let Atom::Capture { atom, .. } = last {
		last = atom;
	}
	let misplaced = rest.iter().any(contains)
		|| (contains(last)
			&& (!matches!(last, Atom::Container(_)) || rule.table() != Table::Block));
	if misplaced {
		Err(RuleErrorKind::MisplacedContainer)
	} else {
		Ok(())
	}
}

/// The run character of the capture a `Same` refers to.
fn same(captured: &Captured, name: &str) -> Result<Option<char>, RuleErrorKind> {
	captured
		.iter()
		.rev()
		.find(|(captured, _)| captured == name)
		.map(|(_, run)| *run)
		.ok_or_else(|| RuleErrorKind::UnknownCapture { name: name.into() })
}

fn check_branch(atoms: &[Atom]) -> Result<(), RuleErrorKind> {
	let first = atoms.first().ok_or(RuleErrorKind::EmptySequence)?;
	if first.decidable() {
		Ok(())
	} else {
		Err(RuleErrorKind::UndecidableBranch {
			atom: first.describe(),
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn lit(text: &str) -> Atom {
		Atom::literal(text)
	}

	fn bold() -> Rule {
		Rule::new(
			"bold",
			vec![
				lit("**"),
				Atom::capture("body", Atom::InlineContent),
				lit("**"),
			],
		)
	}

	fn kind(rules: &mut RuleSet, rule: Rule) -> RuleErrorKind {
		rules.add(rule).unwrap_err().kind
	}

	#[test]
	fn tables_and_openers() {
		let heading = Rule::new(
			"heading",
			vec![
				Atom::LineStart,
				lit("# "),
				Atom::InlineContent,
				Atom::LineEnd,
			],
		);
		assert_eq!(heading.table(), Table::Block);
		assert_eq!(heading.opener().as_deref(), Some("# "));
		assert_eq!(bold().table(), Table::Inline);
		assert_eq!(bold().opener().as_deref(), Some("**"));
		let code = Rule::new("code", vec![Atom::capture("fence", Atom::run('`', 2))]);
		assert_eq!(code.opener().as_deref(), Some("``"));
	}

	#[test]
	fn conflicting_openers() {
		let mut rules = RuleSet::new();
		let first = rules.add(bold()).unwrap();
		let strong = Rule::new("strong", vec![lit("**"), Atom::Text, lit("**")]);
		assert_eq!(
			kind(&mut rules, strong.clone()),
			RuleErrorKind::ConflictingOpener {
				other: "bold".into(),
				opener: "**".into()
			}
		);

		// Same opener in another table, or a prefix of it, is fine.
		rules
			.add(Rule::new(
				"rule",
				vec![Atom::LineStart, lit("**"), Atom::LineEnd],
			))
			.unwrap();
		rules
			.add(Rule::new(
				"italic",
				vec![lit("*"), Atom::InlineContent, lit("*")],
			))
			.unwrap();

		let second = rules.add(strong.replacing("bold")).unwrap();
		assert_ne!(first, second);
		assert!(rules.get(first).is_none());
		assert_eq!(rules.id("strong"), Some(second));
		assert_eq!(rules.id("bold"), None);
		assert_eq!(rules.iter().count(), 3);
	}

	#[test]
	fn dispatch() {
		let mut rules = RuleSet::new();
		let bold = rules.add(bold()).unwrap();
		let italic = rules
			.add(Rule::new(
				"italic",
				vec![lit("*"), Atom::InlineContent, lit("*")],
			))
			.unwrap();
		let hr = rules
			.add(Rule::new(
				"hr",
				vec![Atom::LineStart, lit("***"), Atom::LineEnd],
			))
			.unwrap();

		assert_eq!(rules.dispatch(Table::Inline, "**a**"), Some((bold, 2)));
		assert_eq!(rules.dispatch(Table::Inline, "*a*"), Some((italic, 1)));
		assert_eq!(rules.dispatch(Table::Inline, "***"), Some((bold, 2)));
		assert_eq!(rules.dispatch(Table::Block, "***\n"), Some((hr, 3)));
		assert_eq!(rules.dispatch(Table::Block, "**a"), None);

		let strong = rules
			.add(Rule::new("strong", vec![lit("**"), Atom::Text, lit("**")]).replacing("bold"))
			.unwrap();
		assert_eq!(rules.dispatch(Table::Inline, "**a**"), Some((strong, 2)));
	}

	#[test]
	fn names() {
		let mut rules = RuleSet::new();
		rules.add(bold()).unwrap();
		let other = Rule::new("bold", vec![lit("__"), Atom::Text, lit("__")]);
		assert_eq!(kind(&mut rules, other), RuleErrorKind::DuplicateName);
		let replacing = Rule::new("x", vec![lit("x")]).replacing("nope");
		assert_eq!(
			kind(&mut rules, replacing),
			RuleErrorKind::UnknownReplaced {
				name: "nope".into()
			}
		);
	}

	#[test]
	fn openers_are_literals() {
		let mut rules = RuleSet::new();
		for atoms in [
			vec![],
			vec![Atom::Word],
			vec![Atom::LineStart, Atom::Text, lit("x")],
			vec![lit("")],
		] {
			assert_eq!(
				kind(&mut rules, Rule::new("r", atoms)),
				RuleErrorKind::NoOpener
			);
		}
		rules
			.add(Rule::new("captured", vec![Atom::capture("open", lit("<"))]))
			.unwrap();
	}

	#[test]
	fn open_ended_atoms_need_a_delimiter() {
		let mut rules = RuleSet::new();
		let unterminated = |atom: Atom| Rule::new("r", vec![lit("<"), atom]);
		for atom in [Atom::Text, Atom::InlineContent, Atom::Content] {
			let name = atom.describe();
			assert_eq!(
				kind(&mut rules, unterminated(atom)),
				RuleErrorKind::NotTerminated { atom: name }
			);
		}
		let followed_by_word = Rule::new("r", vec![lit("<"), Atom::Text, Atom::Word]);
		assert_eq!(
			kind(&mut rules, followed_by_word),
			RuleErrorKind::NotTerminated { atom: "Text" }
		);

		// Delimiters can come through captures, alternatives and optional parts.
		rules
			.add(Rule::new(
				"url",
				vec![
					lit("["),
					Atom::InlineContent,
					Atom::capture("sep", lit("](")),
					Atom::Text,
					Atom::optional(vec![lit(" \""), Atom::Text, lit("\"")]),
					Atom::or([vec![lit(")")], vec![lit("|"), Atom::Text, lit(")")]]),
				],
			))
			.unwrap();
	}

	#[test]
	fn content_needs_line_anchored_delimiters() {
		let mut rules = RuleSet::new();
		let unanchored = Rule::new(
			"r",
			vec![Atom::LineStart, lit(":x"), Atom::Content, lit(":end")],
		);
		assert_eq!(
			kind(&mut rules, unanchored),
			RuleErrorKind::ContentNotLineAnchored
		);

		let content = || Atom::capture("body", Atom::Content);
		rules
			.add(Rule::new(
				"layout",
				vec![
					Atom::LineStart,
					lit(":layout"),
					Atom::RestOfLine,
					Atom::LineEnd,
					content(),
					Atom::repeat(
						vec![Atom::LineStart, lit(":next"), Atom::LineEnd, content()],
						0,
						None,
					),
					Atom::LineStart,
					lit(":end"),
					Atom::LineEnd,
				],
			))
			.unwrap();
	}

	#[test]
	fn branches_must_be_decidable() {
		let mut rules = RuleSet::new();
		let repeat = |atoms, min, max| {
			Rule::new("r", vec![lit("<"), Atom::repeat(atoms, min, max), lit(">")])
		};
		assert_eq!(
			kind(&mut rules, repeat(vec![Atom::Text, lit(",")], 0, None)),
			RuleErrorKind::UndecidableBranch { atom: "Text" }
		);
		assert_eq!(
			kind(&mut rules, repeat(vec![], 0, None)),
			RuleErrorKind::EmptySequence
		);
		assert_eq!(
			kind(&mut rules, repeat(vec![Atom::Word], 2, Some(1))),
			RuleErrorKind::InvalidRepeatBounds
		);
		assert_eq!(
			kind(&mut rules, repeat(vec![Atom::Word], 0, Some(0))),
			RuleErrorKind::InvalidRepeatBounds
		);
		let or = Rule::new("r", vec![lit("<"), Atom::or([vec![Atom::RestOfLine]])]);
		assert_eq!(
			kind(&mut rules, or),
			RuleErrorKind::UndecidableBranch { atom: "RestOfLine" }
		);

		// Text inside a repeated body is ended by the body's next start or what follows.
		rules
			.add(repeat(vec![lit(","), Atom::Text], 0, None))
			.unwrap();
	}

	#[test]
	fn error_messages_name_the_rules() {
		let mut rules = RuleSet::new();
		rules.add(bold()).unwrap();
		let error = rules
			.add(Rule::new("strong", vec![lit("**"), Atom::Text, lit("**")]))
			.unwrap_err();
		assert_eq!(
			error.to_string(),
			"rule `strong`: opener `**` is already used by rule `bold` (use `replaces` to override it)"
		);
	}

	#[test]
	fn same_refers_to_an_earlier_capture() {
		let mut rules = RuleSet::new();
		let fence = |atoms: Vec<Atom>| {
			let mut all = vec![Atom::capture("fence", Atom::run('`', 1))];
			all.extend(atoms);
			Rule::new("code", all)
		};
		assert_eq!(
			kind(&mut rules, fence(vec![Atom::Text, Atom::same("nope")])),
			RuleErrorKind::UnknownCapture {
				name: "nope".into()
			}
		);
		assert_eq!(
			kind(
				&mut rules,
				Rule::new(
					"r",
					vec![
						lit("<"),
						Atom::Text,
						Atom::same("x"),
						Atom::capture("x", lit(">")),
					]
				)
			),
			RuleErrorKind::UnknownCapture { name: "x".into() }
		);
		assert_eq!(
			kind(
				&mut rules,
				Rule::new("r", vec![lit("<"), Atom::run('-', 0)])
			),
			RuleErrorKind::EmptyRun
		);
		rules
			.add(fence(vec![Atom::Text, Atom::same("fence")]))
			.unwrap();
	}

	#[test]
	fn containers_end_block_rules() {
		use crate::Prefix;

		let mut rules = RuleSet::new();
		let container = || Atom::Container(Prefix::Literal("> ".into()));
		for atoms in [
			vec![lit(">"), container()],
			vec![Atom::LineStart, lit(">"), container(), Atom::LineEnd],
			vec![
				Atom::LineStart,
				lit(">"),
				Atom::optional(vec![lit("x"), container()]),
			],
		] {
			assert_eq!(
				kind(&mut rules, Rule::new("r", atoms)),
				RuleErrorKind::MisplacedContainer
			);
		}
		rules
			.add(Rule::new(
				"quote",
				vec![
					Atom::LineStart,
					lit("> "),
					Atom::capture("body", container()),
				],
			))
			.unwrap();
	}
}
