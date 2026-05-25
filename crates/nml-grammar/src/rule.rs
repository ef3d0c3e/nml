use std::collections::HashMap;
use std::fmt;

use crate::Atom;
use crate::atom::first_delimiters;

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

	/// The opener: the text the rule is dispatched on.
	pub fn opener(&self) -> Option<&str> {
		let first = match self.table() {
			Table::Block => self.atoms.get(1),
			Table::Inline => self.atoms.first(),
		};
		let mut atom = first?;
		while let Atom::Capture { atom: inner, .. } = atom {
			atom = inner;
		}
		match atom {
			Atom::Literal(text) if !text.is_empty() => Some(text),
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
		}
	}
}

impl std::error::Error for RuleError {}

/// The rules of a grammar, checked when they are added.
#[derive(Debug, Default, Clone)]
pub struct RuleSet {
	/// Indexed by [`RuleId`]; `None` once replaced.
	rules: Vec<Option<Rule>>,
	by_name: HashMap<String, RuleId>,
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
				&& other.opener() == Some(opener)
			{
				return Err(error(RuleErrorKind::ConflictingOpener {
					other: other.name.clone(),
					opener: opener.to_string(),
				}));
			}
		}
		validate(&rule.atoms, &[Vec::new()]).map_err(error)?;

		if let Some(replaced) = replaced {
			let old = self.rules[replaced.0].take().unwrap();
			self.by_name.remove(&old.name);
		}
		let id = RuleId(self.rules.len());
		self.by_name.insert(rule.name.clone(), id);
		self.rules.push(Some(rule));
		Ok(id)
	}

	pub fn get(&self, id: RuleId) -> Option<&Rule> {
		self.rules.get(id.0)?.as_ref()
	}

	pub fn id(&self, name: &str) -> Option<RuleId> {
		self.by_name.get(name).copied()
	}

	/// The active rules, in the order they were added.
	pub fn iter(&self) -> impl Iterator<Item = (RuleId, &Rule)> {
		self.rules
			.iter()
			.enumerate()
			.filter_map(|(index, rule)| Some((RuleId(index), rule.as_ref()?)))
	}
}

/// Checks the atoms of a sequence. `follows` lists what can come after the sequence: one
/// chain of atom sequences per possibility.
fn validate(atoms: &[Atom], follows: &[Vec<&[Atom]>]) -> Result<(), RuleErrorKind> {
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
		match atom {
			Atom::Text | Atom::InlineContent | Atom::Content => {
				for chain in &chains {
					let delimiters =
						first_delimiters(chain, false).ok_or(RuleErrorKind::NotTerminated {
							atom: atom.describe(),
						})?;
					if *atom == Atom::Content && delimiters.iter().any(|d| !d.anchored) {
						return Err(RuleErrorKind::ContentNotLineAnchored);
					}
				}
			}
			Atom::Capture { atom, .. } => validate(std::slice::from_ref(atom), &chains)?,
			Atom::Or(alternatives) => {
				for alternative in alternatives {
					check_branch(alternative)?;
					validate(alternative, &chains)?;
				}
			}
			Atom::Repeat { atoms, min, max } => {
				if max.is_some_and(|max| max == 0 || max < *min) {
					return Err(RuleErrorKind::InvalidRepeatBounds);
				}
				check_branch(atoms)?;
				let mut inner = chains.clone();
				inner.push(vec![atoms]);
				validate(atoms, &inner)?;
			}
			_ => {}
		}
	}
	Ok(())
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
		assert_eq!(heading.opener(), Some("# "));
		assert_eq!(bold().table(), Table::Inline);
		assert_eq!(bold().opener(), Some("**"));
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
}
