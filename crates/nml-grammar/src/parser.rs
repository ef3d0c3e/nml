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

/// The result of parsing a source.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
	pub nodes: Vec<Node>,
	/// Errors leave a usable tree: unclosed constructs end where their parent ends.
	pub diagnostics: Vec<Diagnostic>,
}

/// Parses a source with a rule set.
pub fn parse(rules: &RuleSet, source: Arc<Source>) -> Parsed {
	let mut parser = Parser {
		rules,
		text: source.content(),
		source: &source,
		diagnostics: Vec::new(),
	};
	let nodes = parser.blocks(0, parser.text.len());
	Parsed {
		nodes,
		diagnostics: parser.diagnostics,
	}
}

struct Parser<'a> {
	rules: &'a RuleSet,
	source: &'a Arc<Source>,
	text: &'a str,
	diagnostics: Vec<Diagnostic>,
}

/// The rule being matched.
struct Context {
	rule: RuleId,
	opener: Range<usize>,
	/// The end of the region the rule is matched in: a paragraph for inline rules.
	end: usize,
}

/// A rule that did not match. Its error has been reported.
struct Failed {
	/// Where parsing resumes.
	resume: usize,
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

	/// Parses blocks from `pos`, at the start of a line, to `end`.
	fn blocks(&mut self, mut pos: usize, end: usize) -> Vec<Node> {
		let mut nodes = Vec::new();
		while pos < end {
			if self.is_blank_line(pos, end) {
				pos = self.next_line(pos, end);
				continue;
			}
			if let Some((rule, len)) = self.rules.dispatch(Table::Block, &self.text[pos..end]) {
				match self.rule(rule, pos, pos..pos + len, end) {
					Ok(m) => {
						pos = m.token.end();
						nodes.push(Node::Match(m));
						if !self.is_line_start(pos) && pos < end {
							let kind = DiagnosticKind::ExpectedAtom {
								atom: "LineEnd".into(),
							};
							let location = self.char_token(pos, end);
							self.error(Diagnostic::new(kind, DiagnosticMessage::new(location)));
							pos = self.next_line(pos, end);
						}
					}
					Err(failed) => pos = failed.resume.max(self.next_line(pos, end)),
				}
				continue;
			}

			let (content_end, next) = self.paragraph_end(pos, end);
			let children = self.inline(pos, content_end);
			nodes.push(Node::Paragraph {
				token: self.token(pos..content_end),
				children,
			});
			pos = next;
		}
		nodes
	}

	/// The end of the paragraph's content and the start of the line after it. A paragraph
	/// ends before a blank line or a line where a block rule starts.
	fn paragraph_end(&self, mut pos: usize, end: usize) -> (usize, usize) {
		loop {
			let content_end = self.line_end(pos, end);
			let next = self.next_line(pos, end);
			if next >= end
				|| self.is_blank_line(next, end)
				|| self
					.rules
					.dispatch(Table::Block, &self.text[next..end])
					.is_some()
			{
				return (content_end, next);
			}
			pos = next;
		}
	}

	/// Parses inline content from `start` to `end`.
	fn inline(&mut self, start: usize, end: usize) -> Vec<Node> {
		let mut nodes = Vec::new();
		let mut text_start = start;
		let mut pos = start;
		while pos < end {
			let Some((rule, len)) = self.rules.dispatch(Table::Inline, &self.text[pos..end]) else {
				pos += self.text[pos..].chars().next().unwrap().len_utf8();
				continue;
			};
			match self.rule(rule, pos, pos..pos + len, end) {
				Ok(m) => {
					if text_start < pos {
						nodes.push(Node::Text(self.token(text_start..pos)));
					}
					pos = m.token.end();
					text_start = pos;
					nodes.push(Node::Match(m));
				}
				// The opener is kept as text, so the tree still covers the source.
				Err(failed) => pos = failed.resume,
			}
		}
		if text_start < end {
			nodes.push(Node::Text(self.token(text_start..end)));
		}
		nodes
	}

	/// Matches a rule at `start`, whose opener covers `opener`.
	fn rule(
		&mut self,
		rule: RuleId,
		start: usize,
		opener: Range<usize>,
		end: usize,
	) -> Result<RuleMatch, Failed> {
		let resume = opener.end;
		let context = Context { rule, opener, end };
		let mut captures = Vec::new();
		match self.seq(&context, self.rules.ops(rule), start, &mut captures) {
			Ok(pos) => Ok(RuleMatch {
				rule,
				token: self.token(start..pos),
				captures,
			}),
			Err(diagnostic) => {
				self.error(diagnostic);
				Err(Failed { resume })
			}
		}
	}

	fn seq(
		&mut self,
		context: &Context,
		ops: &[Op],
		mut pos: usize,
		captures: &mut Vec<Capture>,
	) -> Result<usize, Diagnostic> {
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
	) -> Result<(usize, Value), Diagnostic> {
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
				while max.is_none_or(|max| count < max) && self.starts(&ops[0], pos, end) {
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
			Op::InlineContent(_) | Op::Content(_) => todo!("content atoms"),
			Op::Capture { name, op } => {
				let index = captures.len();
				let (after, value) = self.op(context, op, pos, captures)?;
				captures.insert(
					index,
					Capture {
						name: name.clone(),
						token: self.token(pos..after),
						value,
					},
				);
				Ok((after, Value::Span))
			}
			Op::Literal(_) | Op::LineStart => Err(self.expected(op, pos, end)),
		}
	}

	/// Whether `op` can start at `pos`; for the first atom of an `Or` or `Repeat` branch.
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
				.any(|ops| self.starts(&ops[0], pos, end)),
			Op::Repeat { ops, .. } => self.starts(&ops[0], pos, end),
			Op::Text(_) | Op::RestOfLine | Op::InlineContent(_) | Op::Content(_) => true,
		}
	}

	/// Verbatim text up to one of `delimiters`.
	fn text_until(
		&mut self,
		context: &Context,
		delimiters: &[DelimiterPattern],
		start: usize,
	) -> Result<(usize, Value), Diagnostic> {
		let end = context.end;
		let mut pos = start;
		loop {
			if delimiters.iter().any(|d| self.delimiter_at(d, pos, end)) {
				return Ok((pos, Value::Span));
			}
			match self.text[pos..end].chars().next() {
				Some(c) => pos += c.len_utf8(),
				None => return Err(self.unclosed(context, delimiters, pos)),
			}
		}
	}

	fn delimiter_at(&self, delimiter: &DelimiterPattern, pos: usize, end: usize) -> bool {
		if delimiter.anchored && !self.is_line_start(pos) {
			return false;
		}
		match &delimiter.kind {
			DelimiterKind::Literal(text) => self.text[pos..end].starts_with(text.as_str()),
			DelimiterKind::LineEnd => self.line_break(pos, end).is_some(),
		}
	}

	/// The error for a construct whose delimiter is missing: reported at the opener, with the
	/// place where it had to end, and a fix that escapes the opener.
	fn unclosed(
		&self,
		context: &Context,
		delimiters: &[DelimiterPattern],
		pos: usize,
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
			self.char_token(pos, context.end),
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

	fn expected(&self, op: &Op, pos: usize, end: usize) -> Diagnostic {
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
		Diagnostic::new(kind, DiagnosticMessage::new(self.char_token(pos, end)))
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
					format!(
						"{}({})",
						rules.get(m.rule).unwrap().name,
						captures.join(" ")
					)
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
		let diagnostics = check("####### x\nnext", r#"P["next"]"#);
		assert_diagnostics!(diagnostics, [1001 @ 6..7]);

		let diagnostics = check(":set 1 = 2", "");
		assert_diagnostics!(diagnostics, [1003 @ 5..6]);

		// The opener is kept as text and parsing resumes after it.
		let diagnostics = check("a {} `b`", r#"P["a {} " code(code="b")]"#);
		assert_diagnostics!(diagnostics, [1003 @ 3..4]);
	}

	#[test]
	fn unclosed_text() {
		let rules = rules();
		let parsed = parse(&rules, source("a `b\n\nc"));
		assert_eq!(show(&rules, &parsed.nodes), r#"P["a `b"] P["c"]"#);
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
}
