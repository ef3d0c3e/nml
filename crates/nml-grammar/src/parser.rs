use std::ops::Range;
use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_diagnostics::Edit;
use nml_diagnostics::Fix;
use nml_diagnostics::MessageContent;
use nml_source::Source;
use nml_source::Token;

use crate::Capture;
use crate::Node;
use crate::RuleId;
use crate::RuleMatch;
use crate::RuleSet;
use crate::Table;
use crate::Value;
use crate::atom::DelimiterKind;
use crate::atom::DelimiterPattern;
use crate::atom::Op;

/// The deepest nesting of content atoms, which bounds the parser's recursion.
pub const MAX_NESTING: usize = 128;

/// The result of parsing a source.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
	pub nodes: Vec<Node>,
	/// Errors leave a usable tree: a rule that fails ends where it failed, and an unclosed
	/// construct ends where its parent ends.
	pub diagnostics: Vec<Diagnostic>,
}

/// Parses a source with a rule set.
pub fn parse(rules: &RuleSet, source: Arc<Source>) -> Parsed {
	let mut parser = Parser {
		rules,
		text: source.content(),
		source: &source,
		frames: Vec::new(),
		diagnostics: Vec::new(),
	};
	let (nodes, _, _) = parser.blocks(0, parser.text.len());
	Parsed {
		nodes,
		diagnostics: parser.diagnostics,
	}
}

struct Parser<'a> {
	rules: &'a RuleSet,
	source: &'a Arc<Source>,
	text: &'a str,
	/// The expected-delimiter stack: one frame per content atom being parsed.
	frames: Vec<Frame>,
	diagnostics: Vec<Diagnostic>,
}

/// The delimiters that end a content atom.
struct Frame {
	delimiters: Vec<DelimiterPattern>,
	/// `InlineContent`, as opposed to `Content`.
	inline: bool,
}

/// Why parsing a sequence of nodes stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
	/// The end of the region.
	End,
	/// A delimiter of this frame.
	Frame(usize),
}

/// The rule being matched.
struct Context {
	rule: RuleId,
	opener: Range<usize>,
	/// The end of the region the rule is matched in: a paragraph for inline rules.
	end: usize,
}

/// An atom that did not match.
struct Error {
	diagnostic: Diagnostic,
	/// Where the match stopped.
	pos: usize,
	/// The partial value of an unclosed atom, kept in the tree.
	value: Option<Value>,
}

impl Error {
	fn new(diagnostic: Diagnostic, pos: usize) -> Self {
		Self {
			diagnostic,
			pos,
			value: None,
		}
	}
}

impl Parser<'_> {
	fn token(&self, range: Range<usize>) -> Token {
		Token::new(self.source.clone(), range)
	}

	/// The token of the character at `pos`, or an empty token at the end.
	fn char_token(&self, pos: usize, end: usize) -> Token {
		let len = self.text[pos..end].chars().next().map_or(0, char::len_utf8);
		self.token(pos..pos + len)
	}

	fn is_line_start(&self, pos: usize) -> bool {
		pos == 0 || self.text.as_bytes()[pos - 1] == b'\n'
	}

	/// The end of the line at `pos`, before its line break.
	fn line_end(&self, pos: usize, end: usize) -> usize {
		let le = self.text[pos..end].find('\n').map_or(end, |i| pos + i);
		if le > pos && self.text.as_bytes()[le - 1] == b'\r' {
			le - 1
		} else {
			le
		}
	}

	/// The start of the next line, or `end`.
	fn next_line(&self, pos: usize, end: usize) -> usize {
		self.text[pos..end].find('\n').map_or(end, |i| pos + i + 1)
	}

	fn is_blank_line(&self, pos: usize, end: usize) -> bool {
		self.text[pos..self.line_end(pos, end)]
			.chars()
			.all(|c| c == ' ' || c == '\t')
	}

	/// The end of the paragraph-like region at `pos`: the end of the line before the next
	/// blank line.
	fn before_blank_line(&self, mut pos: usize, end: usize) -> usize {
		loop {
			let next = self.next_line(pos, end);
			if next >= end || self.is_blank_line(next, end) {
				return self.line_end(pos, end);
			}
			pos = next;
		}
	}

	/// A line break at `pos`: its length, or `Some(0)` at the end of the region.
	fn line_break(&self, pos: usize, end: usize) -> Option<usize> {
		let rest = &self.text[pos..end];
		if rest.is_empty() {
			Some(0)
		} else if rest.starts_with('\n') {
			Some(1)
		} else if rest.starts_with("\r\n") {
			Some(2)
		} else {
			None
		}
	}

	fn error(&mut self, diagnostic: Diagnostic) {
		self.diagnostics.push(diagnostic);
	}

	/// The innermost frame with a delimiter at `pos`, and the delimiter's length. Only the
	/// frames of the current phase count: inline frames inside inline content, block frames
	/// between blocks.
	fn closing(&self, pos: usize, end: usize, inline: bool) -> Option<(usize, usize)> {
		for (index, frame) in self.frames.iter().enumerate().rev() {
			if frame.inline != inline {
				break;
			}
			if let Some(len) = frame
				.delimiters
				.iter()
				.find_map(|d| self.delimiter_at(d, pos, end))
			{
				return Some((index, len));
			}
		}
		None
	}

	/// The length of a delimiter at `pos`.
	fn delimiter_at(&self, delimiter: &DelimiterPattern, pos: usize, end: usize) -> Option<usize> {
		if delimiter.anchored && !self.is_line_start(pos) {
			return None;
		}
		match &delimiter.kind {
			DelimiterKind::Literal(text) => self.text[pos..end]
				.starts_with(text.as_str())
				.then_some(text.len()),
			DelimiterKind::LineEnd => self.line_break(pos, end),
		}
	}

	/// The rule to dispatch at `pos`, unless a delimiter closes the current frame there. An
	/// opener wins only if it is longer than the delimiter: inside `*…*`, `**` opens bold,
	/// but inside `**…**`, `**` closes.
	fn next(&self, table: Table, pos: usize, end: usize) -> Result<Option<(RuleId, usize)>, usize> {
		let closing = self.closing(pos, end, table == Table::Inline);
		let opener = self.rules.dispatch(table, &self.text[pos..end]);
		match (closing, opener) {
			(Some((frame, len)), opener) if opener.is_none_or(|(_, l)| l <= len) => Err(frame),
			(_, opener) => Ok(opener),
		}
	}

	/// Parses blocks from `pos`, at the start of a line, up to `end` or a delimiter of a
	/// block frame.
	fn blocks(&mut self, mut pos: usize, end: usize) -> (Vec<Node>, usize, Stop) {
		let mut nodes = Vec::new();
		loop {
			if pos >= end {
				return (nodes, end, Stop::End);
			}
			if self.is_blank_line(pos, end) {
				pos = self.next_line(pos, end);
				continue;
			}
			let rule = match self.next(Table::Block, pos, end) {
				Ok(rule) => rule,
				Err(frame) => return (nodes, pos, Stop::Frame(frame)),
			};
			if let Some((rule, len)) = rule {
				let m = self.rule(rule, pos..pos + len, end);
				let complete = m.complete;
				pos = m.token.end();
				nodes.push(Node::Match(m));
				if !complete {
					// The rest of the line is skipped.
					if !self.is_line_start(pos) {
						pos = self.next_line(pos, end);
					}
				} else if !self.is_line_start(pos) && pos < end {
					let kind = DiagnosticKind::ExpectedAtom {
						atom: "LineEnd".into(),
					};
					let location = self.char_token(pos, end);
					self.error(Diagnostic::new(kind, DiagnosticMessage::new(location)));
					pos = self.next_line(pos, end);
				}
				continue;
			}

			let (content_end, next) = self.paragraph_end(pos, end);
			let (children, _, _) = self.inline(pos, content_end);
			nodes.push(Node::Paragraph {
				token: self.token(pos..content_end),
				children,
			});
			pos = next;
		}
	}

	/// The end of the paragraph's content and the start of the line after it. A paragraph
	/// ends before a blank line, a line where a block rule starts, or a delimiter of a block
	/// frame.
	fn paragraph_end(&self, mut pos: usize, end: usize) -> (usize, usize) {
		loop {
			let content_end = self.line_end(pos, end);
			let next = self.next_line(pos, end);
			if next >= end
				|| self.is_blank_line(next, end)
				|| !matches!(self.next(Table::Block, next, end), Ok(None))
			{
				return (content_end, next);
			}
			pos = next;
		}
	}

	/// Parses inline content from `start` up to `end` or a delimiter of an inline frame.
	fn inline(&mut self, start: usize, end: usize) -> (Vec<Node>, usize, Stop) {
		let mut nodes = Vec::new();
		let mut text_start = start;
		let mut pos = start;
		let stop = loop {
			let rule = match self.next(Table::Inline, pos, end) {
				Ok(rule) => rule,
				Err(frame) => break Stop::Frame(frame),
			};
			if pos >= end {
				break Stop::End;
			}
			let Some((rule, len)) = rule else {
				pos += self.text[pos..].chars().next().unwrap().len_utf8();
				continue;
			};
			if text_start < pos {
				nodes.push(Node::Text(self.token(text_start..pos)));
			}
			let m = self.rule(rule, pos..pos + len, end);
			pos = m.token.end();
			text_start = pos;
			nodes.push(Node::Match(m));
		};
		if text_start < pos {
			nodes.push(Node::Text(self.token(text_start..pos)));
		}
		(nodes, pos, stop)
	}

	/// Matches a rule whose opener covers `opener`. A rule that fails is reported, and its
	/// partial match ends where it failed.
	fn rule(&mut self, rule: RuleId, opener: Range<usize>, end: usize) -> RuleMatch {
		let start = opener.start;
		let context = Context { rule, opener, end };
		let mut captures = Vec::new();
		let (pos, complete) = match self.seq(&context, self.rules.ops(rule), start, &mut captures) {
			Ok(pos) => (pos, true),
			Err(error) => {
				self.error(error.diagnostic);
				// Resume after the opener at least, so that parsing always advances.
				(error.pos.max(context.opener.end), false)
			}
		};
		RuleMatch {
			rule,
			token: self.token(start..pos),
			captures,
			complete,
		}
	}

	fn seq(
		&mut self,
		context: &Context,
		ops: &[Op],
		mut pos: usize,
		captures: &mut Vec<Capture>,
	) -> Result<usize, Error> {
		for op in ops {
			pos = self.op(context, op, pos, captures)?.0;
		}
		Ok(pos)
	}

	/// Matches one atom: the position after it and its value.
	fn op(
		&mut self,
		context: &Context,
		op: &Op,
		pos: usize,
		captures: &mut Vec<Capture>,
	) -> Result<(usize, Value), Error> {
		let end = context.end;
		let rest = &self.text[pos..end];
		let matched = |len: usize| Ok((pos + len, Value::Span));
		match op {
			Op::Literal(text) if rest.starts_with(text.as_str()) => matched(text.len()),
			Op::LineStart if self.is_line_start(pos) => matched(0),
			Op::LineEnd => match self.line_break(pos, end) {
				Some(len) => matched(len),
				None => Err(self.expected(op, pos, end)),
			},
			Op::Or(alternatives) => {
				match alternatives
					.iter()
					.find(|ops| self.starts(&ops[0], pos, end))
				{
					Some(ops) => Ok((self.seq(context, ops, pos, captures)?, Value::Span)),
					None => Err(self.expected(op, pos, end)),
				}
			}
			Op::Repeat { ops, min, max } => {
				let mut count = 0;
				let mut pos = pos;
				while max.is_none_or(|max| count < max) && self.starts_seq(ops, pos, end) {
					pos = self.seq(context, ops, pos, captures)?;
					count += 1;
				}
				if count < *min {
					return Err(self.expected(&ops[0], pos, end));
				}
				Ok((pos, Value::Span))
			}
			Op::Number { signed, decimal } => match number(rest, *signed, *decimal) {
				Some((len, value)) => Ok((pos + len, Value::Number(value))),
				None => Err(self.expected(op, pos, end)),
			},
			Op::Word => match word(rest) {
				0 => Err(self.expected(op, pos, end)),
				len => matched(len),
			},
			Op::RestOfLine => Ok((self.line_end(pos, end), Value::Span)),
			Op::Text(delimiters) => self.text_until(context, delimiters, pos),
			Op::InlineContent(delimiters) => {
				let limit = self.before_blank_line(pos, end);
				self.content(context, delimiters, true, pos, limit)
			}
			Op::Content(delimiters) => self.content(context, delimiters, false, pos, end),
			Op::Capture { name, op } => {
				let index = captures.len();
				let source = self.source;
				let capture = |after: usize, value| Capture {
					name: name.clone(),
					token: Token::new(source.clone(), pos..after),
					value,
				};
				match self.op(context, op, pos, captures) {
					Ok((after, value)) => {
						captures.insert(index, capture(after, value));
						Ok((after, Value::Span))
					}
					Err(mut error) => {
						if let Some(value) = error.value.take() {
							captures.insert(index, capture(error.pos, value));
						}
						Err(error)
					}
				}
			}
			Op::Literal(_) | Op::LineStart => Err(self.expected(op, pos, end)),
		}
	}

	/// Whether a branch of an `Or` or `Repeat` can start at `pos`. A leading `LineStart` is
	/// checked together with the atom after it.
	fn starts_seq(&self, ops: &[Op], pos: usize, end: usize) -> bool {
		match ops {
			[Op::LineStart, rest @ ..] if !rest.is_empty() => {
				self.is_line_start(pos) && self.starts_seq(rest, pos, end)
			}
			[first, ..] => self.starts(first, pos, end),
			[] => true,
		}
	}

	/// Whether `op` can start at `pos`.
	fn starts(&self, op: &Op, pos: usize, end: usize) -> bool {
		let rest = &self.text[pos..end];
		match op {
			Op::Literal(text) => rest.starts_with(text.as_str()),
			Op::LineStart => self.is_line_start(pos),
			Op::LineEnd => self.line_break(pos, end).is_some(),
			Op::Word => word(rest) > 0,
			Op::Number { signed, decimal } => number(rest, *signed, *decimal).is_some(),
			Op::Capture { op, .. } => self.starts(op, pos, end),
			Op::Or(alternatives) => alternatives
				.iter()
				.any(|ops| self.starts_seq(ops, pos, end)),
			Op::Repeat { ops, .. } => self.starts_seq(ops, pos, end),
			Op::Text(_) | Op::RestOfLine | Op::InlineContent(_) | Op::Content(_) => true,
		}
	}

	/// Verbatim text up to one of `delimiters`. Outer frames do not end it.
	fn text_until(
		&mut self,
		context: &Context,
		delimiters: &[DelimiterPattern],
		start: usize,
	) -> Result<(usize, Value), Error> {
		let end = context.end;
		let mut pos = start;
		loop {
			if delimiters
				.iter()
				.any(|d| self.delimiter_at(d, pos, end).is_some())
			{
				return Ok((pos, Value::Span));
			}
			match self.text[pos..end].chars().next() {
				Some(c) => pos += c.len_utf8(),
				None => {
					return Err(Error {
						diagnostic: self.unclosed(context, delimiters, pos, end),
						pos,
						value: Some(Value::Span),
					});
				}
			}
		}
	}

	/// Parsed content up to one of `delimiters`, with a frame for them on the stack. If an
	/// outer frame's delimiter or the end of the region comes first, the construct is
	/// unclosed: it ends there, and the outer frame continues.
	fn content(
		&mut self,
		context: &Context,
		delimiters: &[DelimiterPattern],
		inline: bool,
		start: usize,
		end: usize,
	) -> Result<(usize, Value), Error> {
		if self.frames.len() >= MAX_NESTING {
			let kind = DiagnosticKind::NestingTooDeep { limit: MAX_NESTING };
			let location = self.token(context.opener.clone());
			return Err(Error::new(
				Diagnostic::new(kind, DiagnosticMessage::new(location)),
				start,
			));
		}
		self.frames.push(Frame {
			delimiters: delimiters.to_vec(),
			inline,
		});
		let own = self.frames.len() - 1;
		let (nodes, pos, stop) = if inline {
			self.inline(start, end)
		} else {
			self.blocks(start, end)
		};
		self.frames.pop();
		if stop == Stop::Frame(own) {
			Ok((pos, Value::Nodes(nodes)))
		} else {
			Err(Error {
				diagnostic: self.unclosed(context, delimiters, pos, end),
				pos,
				value: Some(Value::Nodes(nodes)),
			})
		}
	}

	/// The error for a construct whose delimiter is missing: reported at the opener, with the
	/// place where it had to end, and a fix that escapes the opener.
	fn unclosed(
		&self,
		context: &Context,
		delimiters: &[DelimiterPattern],
		pos: usize,
		end: usize,
	) -> Diagnostic {
		let rule = &self.rules.get(context.rule).unwrap().name;
		let token = delimiters
			.iter()
			.find_map(|d| match &d.kind {
				DelimiterKind::Literal(text) => Some(text.clone()),
				DelimiterKind::LineEnd => None,
			})
			.unwrap_or_default();
		let opener = self.token(context.opener.clone());
		let escaped = opener.content().chars().fold(String::new(), |mut s, c| {
			if c.is_ascii_punctuation() {
				s.push('\\');
			}
			s.push(c);
			s
		});
		Diagnostic::new(
			DiagnosticKind::ExpectedToken { token },
			DiagnosticMessage::fluent(opener.clone(), "opened").arg("style", rule.as_str()),
		)
		.with(DiagnosticMessage::fluent(
			self.char_token(pos, end),
			"ended",
		))
		.with_fix(Fix {
			title: MessageContent::Fluent {
				attribute: "fix-escape",
				args: Vec::new(),
			},
			edits: vec![Edit {
				location: opener,
				replacement: escaped,
			}],
		})
	}

	fn expected(&self, op: &Op, pos: usize, end: usize) -> Error {
		let kind = match op {
			Op::Literal(text) => DiagnosticKind::ExpectedToken {
				token: text.clone(),
			},
			Op::Or(alternatives) => return self.expected(&alternatives[0][0], pos, end),
			Op::Repeat { ops, .. } => return self.expected(&ops[0], pos, end),
			Op::Capture { op, .. } => return self.expected(op, pos, end),
			op => DiagnosticKind::ExpectedAtom {
				atom: op.describe().into(),
			},
		};
		let diagnostic = Diagnostic::new(kind, DiagnosticMessage::new(self.char_token(pos, end)));
		Error::new(diagnostic, pos)
	}
}

/// The length of the word at the start of `text`, or 0.
fn word(text: &str) -> usize {
	let mut chars = text.char_indices();
	match chars.next() {
		Some((_, c)) if c.is_alphabetic() || c == '_' => {}
		_ => return 0,
	}
	chars
		.find(|(_, c)| !(c.is_alphanumeric() || *c == '_' || *c == '-'))
		.map_or(text.len(), |(i, _)| i)
}

/// The number at the start of `text`: its length and value.
fn number(text: &str, signed: bool, decimal: bool) -> Option<(usize, f64)> {
	let bytes = text.as_bytes();
	let mut len = usize::from(signed && bytes.first() == Some(&b'-'));
	let digits = |from: usize| {
		bytes[from..]
			.iter()
			.take_while(|b| b.is_ascii_digit())
			.count()
	};
	let integer = digits(len);
	if integer == 0 {
		return None;
	}
	len += integer;
	if decimal && bytes.get(len) == Some(&b'.') {
		let fraction = digits(len + 1);
		if fraction > 0 {
			len += 1 + fraction;
		}
	}
	Some((len, text[..len].parse().unwrap()))
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_source::Url;

	use crate::Atom;
	use crate::Rule;

	use super::*;

	pub(crate) fn source(text: &str) -> Arc<Source> {
		Arc::new(Source::new(
			Url::parse("file:///test.nml").unwrap(),
			text.into(),
			0,
		))
	}

	fn lit(text: &str) -> Atom {
		Atom::literal(text)
	}

	/// A compact rendering of a tree: `rule(capture=value ...)`, paragraphs as `P[...]`.
	pub(crate) fn show(rules: &RuleSet, nodes: &[Node]) -> String {
		let mut out = Vec::new();
		for node in nodes {
			out.push(match node {
				Node::Text(token) => format!("{:?}", token.content()),
				Node::Paragraph { children, .. } => format!("P[{}]", show(rules, children)),
				Node::Match(m) => {
					let captures: Vec<_> = m
						.captures
						.iter()
						.map(|capture| {
							let value = match &capture.value {
								Value::Span => format!("{:?}", capture.token.content()),
								Value::Number(n) => n.to_string(),
								Value::Nodes(nodes) => format!("[{}]", show(rules, nodes)),
							};
							format!("{}={value}", capture.name)
						})
						.collect();
					let mark = if m.complete { "" } else { "!" };
					let name = &rules.get(m.rule).unwrap().name;
					format!("{name}{mark}({})", captures.join(" "))
				}
			});
		}
		out.join(" ")
	}

	fn rules() -> RuleSet {
		let mut rules = RuleSet::new();
		let rule = |name, atoms| Rule::new(name, atoms);
		for rule in [
			rule(
				"heading",
				vec![
					Atom::LineStart,
					lit("#"),
					Atom::capture("deeper", Atom::repeat(vec![lit("#")], 0, Some(5))),
					lit(" "),
					Atom::capture("title", Atom::RestOfLine),
					Atom::LineEnd,
				],
			),
			rule(
				"set",
				vec![
					Atom::LineStart,
					lit(":set"),
					lit(" "),
					Atom::capture("name", Atom::Word),
					lit(" = "),
					Atom::or([
						vec![Atom::capture(
							"number",
							Atom::Number {
								signed: true,
								decimal: true,
							},
						)],
						vec![lit("\""), Atom::capture("string", Atom::Text), lit("\"")],
					]),
					Atom::LineEnd,
				],
			),
			rule(
				"code",
				vec![lit("`"), Atom::capture("code", Atom::Text), lit("`")],
			),
			rule(
				"link",
				vec![
					lit("<"),
					Atom::capture("url", Atom::Text),
					Atom::optional(vec![lit("|"), Atom::capture("title", Atom::Text)]),
					lit(">"),
				],
			),
			rule(
				"list",
				vec![
					lit("{"),
					Atom::repeat(
						vec![
							Atom::capture("item", Atom::Word),
							Atom::optional(vec![lit(", ")]),
						],
						1,
						None,
					),
					lit("}"),
				],
			),
			rule(
				"bold",
				vec![
					lit("**"),
					Atom::capture("body", Atom::InlineContent),
					lit("**"),
				],
			),
			rule(
				"italic",
				vec![
					lit("*"),
					Atom::capture("body", Atom::InlineContent),
					lit("*"),
				],
			),
			rule(
				"url",
				vec![
					lit("["),
					Atom::capture("text", Atom::InlineContent),
					lit("]("),
					Atom::capture("url", Atom::Text),
					lit(")"),
				],
			),
			rule(
				"title",
				vec![
					Atom::LineStart,
					lit("= "),
					Atom::capture("body", Atom::InlineContent),
					Atom::LineEnd,
				],
			),
			rule(
				"layout",
				vec![
					Atom::LineStart,
					lit(":layout"),
					Atom::LineEnd,
					Atom::capture("body", Atom::Content),
					Atom::repeat(
						vec![
							Atom::LineStart,
							lit(":next"),
							Atom::LineEnd,
							Atom::capture("body", Atom::Content),
						],
						0,
						None,
					),
					Atom::LineStart,
					lit(":end"),
					Atom::LineEnd,
				],
			),
		] {
			rules.add(rule).unwrap();
		}
		rules
	}

	fn check(text: &str, expected: &str) -> Vec<Diagnostic> {
		let rules = rules();
		let parsed = parse(&rules, source(text));
		assert_eq!(show(&rules, &parsed.nodes), expected);
		parsed.diagnostics
	}

	#[test]
	fn paragraphs() {
		let diagnostics = check(
			"first line\nsecond line\n\n\n  \nthird\n",
			r#"P["first line\nsecond line"] P["third"]"#,
		);
		assert_diagnostics!(diagnostics, []);
		check("", "");
		check("a\r\nb\r\n\r\nc", r#"P["a\r\nb"] P["c"]"#);
	}

	#[test]
	fn block_rules() {
		let diagnostics = check(
			"## Title\ntext\n# Next\n:set x = -1.5\n:set y = \"a b\"\n",
			r###"heading(deeper="#" title="Title") P["text"] heading(deeper="" title="Next") set(name="x" number=-1.5) set(name="y" string="a b")"###,
		);
		assert_diagnostics!(diagnostics, []);
	}

	#[test]
	fn block_rules_interrupt_paragraphs() {
		check(
			"text\n# Title",
			r#"P["text"] heading(deeper="" title="Title")"#,
		);
	}

	#[test]
	fn inline_rules() {
		let diagnostics = check(
			"a `b` <x|y> <z> {a, b, c} é",
			r#"P["a " code(code="b") " " link(url="x" title="y") " " link(url="z") " " list(item="a" item="b" item="c") " é"]"#,
		);
		assert_diagnostics!(diagnostics, []);
	}

	#[test]
	fn failures() {
		// Too many `#`: the `Repeat` stops at 6, then `" "` is missing.
		let diagnostics = check(
			"####### x\nnext",
			r######"heading!(deeper="#####") P["next"]"######,
		);
		assert_diagnostics!(diagnostics, [1001 @ 6..7]);

		let diagnostics = check(":set 1 = 2", "set!()");
		assert_diagnostics!(diagnostics, [1003 @ 5..6]);

		// The partial match ends where the rule failed, and parsing resumes there.
		let diagnostics = check("a {} `b`", r#"P["a " list!() "} " code(code="b")]"#);
		assert_diagnostics!(diagnostics, [1003 @ 3..4]);
	}

	#[test]
	fn unclosed_text() {
		let rules = rules();
		let parsed = parse(&rules, source("a `b\n\nc"));
		assert_eq!(
			show(&rules, &parsed.nodes),
			r#"P["a " code!(code="b")] P["c"]"#
		);
		assert_diagnostics!(parsed.diagnostics, [1001 @ 2..3]);
		let diagnostic = &parsed.diagnostics[0];
		assert_eq!(diagnostic.messages()[1].location.range(), 4..4);
		assert_eq!(diagnostic.fixes[0].edits[0].replacement, "\\`");
	}

	#[test]
	fn trailing_text_after_block_rule() {
		let mut rules = rules();
		rules
			.add(Rule::new("hr", vec![Atom::LineStart, lit("---")]))
			.unwrap();
		let parsed = parse(&rules, source("--- x\ntext"));
		assert_eq!(show(&rules, &parsed.nodes), r#"hr() P["text"]"#);
		assert_diagnostics!(parsed.diagnostics, [1003 @ 3..4]);
	}

	#[test]
	fn numbers_and_words() {
		assert_eq!(number("12a", false, false), Some((2, 12.0)));
		assert_eq!(number("-1.5", true, true), Some((4, -1.5)));
		assert_eq!(number("-1", false, true), None);
		assert_eq!(number("1.", true, true), Some((1, 1.0)));
		assert_eq!(number("1.5", true, false), Some((1, 1.0)));
		assert_eq!(word("a-b_c d"), 5);
		assert_eq!(word("_x"), 2);
		assert_eq!(word("1x"), 0);
	}

	#[test]
	fn nesting() {
		let diagnostics = check(
			"*a **b** c* **a *b* c** **`a**b`**",
			r#"P[italic(body=["a " bold(body=["b"]) " c"]) " " bold(body=["a " italic(body=["b"]) " c"]) " " bold(body=[code(code="a**b")])]"#,
		);
		assert_diagnostics!(diagnostics, []);
	}

	#[test]
	fn unclosed_inside_a_frame() {
		// `](` belongs to the link: the bold is unclosed and ends there.
		let rules = rules();
		let parsed = parse(&rules, source("[a **b](url) c"));
		assert_eq!(
			show(&rules, &parsed.nodes),
			r#"P[url(text=["a " bold!(body=["b"])] url="url") " c"]"#
		);
		assert_diagnostics!(parsed.diagnostics, [1001 @ 3..5]);
		assert_eq!(parsed.diagnostics[0].messages()[1].location.range(), 6..7);
		assert_eq!(
			parsed.diagnostics[0].fixes[0].edits[0].replacement,
			"\\*\\*"
		);
	}

	#[test]
	fn unclosed_at_paragraph_end() {
		let diagnostics = check(
			"**a *b\n\nnext",
			r#"P[bold!(body=["a " italic!(body=["b"])])] P["next"]"#,
		);
		assert_diagnostics!(diagnostics, [1001 @ 4..5, 1001 @ 0..2]);

		// The title's line end belongs to an outer frame: the bold ends there.
		let diagnostics = check(
			"= a **b\nc",
			r#"title(body=["a " bold!(body=["b"])]) P["c"]"#,
		);
		assert_diagnostics!(diagnostics, [1001 @ 4..6]);
		let diagnostics = check(
			"= a *b*\nc",
			r#"title(body=["a " italic(body=["b"])]) P["c"]"#,
		);
		assert_diagnostics!(diagnostics, []);
	}

	#[test]
	fn content() {
		let diagnostics = check(
			":layout\none\n\ntwo\n:next\n# three\n:end\nafter",
			r#"layout(body=[P["one"] P["two"]] body=[heading(deeper="" title="three")]) P["after"]"#,
		);
		assert_diagnostics!(diagnostics, []);

		let diagnostics = check(
			":layout\na\n:layout\nb\n:next\nc\n:end\n:end\n",
			r#"layout(body=[P["a"] layout(body=[P["b"]] body=[P["c"]])])"#,
		);
		assert_diagnostics!(diagnostics, []);
	}

	#[test]
	fn unclosed_content() {
		let rules = rules();
		let parsed = parse(&rules, source(":layout\na\n:layout\nb\n:end\n"));
		assert_eq!(
			show(&rules, &parsed.nodes),
			r#"layout!(body=[P["a"] layout(body=[P["b"]])])"#
		);
		assert_diagnostics!(parsed.diagnostics, [1001 @ 0..7]);
		assert_eq!(
			parsed.diagnostics[0].kind,
			DiagnosticKind::ExpectedToken {
				token: ":end".into()
			}
		);
	}

	#[test]
	fn nesting_is_bounded() {
		let rules = rules();
		let text = "[".repeat(10_000);
		let parsed = parse(&rules, source(&text));
		// One error per opener: past the limit, each `[` fails on its own.
		assert_eq!(parsed.diagnostics.len(), text.len());
		assert_eq!(parsed.diagnostics[0].kind.id(), 1004);
		assert_eq!(
			parsed.diagnostics[0].primary().location.range(),
			MAX_NESTING..MAX_NESTING + 1
		);
	}
}
