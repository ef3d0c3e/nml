use std::ops::Range;
use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_diagnostics::Edit;
use nml_diagnostics::Fix;
use nml_diagnostics::MessageContent;
use nml_source::Bias;
use nml_source::Source;
use nml_source::Token;
use nml_source::Url;
use nml_source::VirtualSourceBuilder;

use crate::Capture;
use crate::Node;
use crate::OptionEntry;
use crate::OptionSchema;
use crate::OptionValue;
use crate::Options;
use crate::Prefix;
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
	parse_from(rules, source, 0)
}

/// Parses a source from `start`, the start of a line, to its end.
///
/// # Panics
///
/// If `start` is not the start of a line.
pub fn parse_from(rules: &RuleSet, source: Arc<Source>, start: usize) -> Parsed {
	let mut parser = Parser {
		rules,
		text: source.content(),
		source: &source,
		frames: Vec::new(),
		depth: 0,
		diagnostics: Vec::new(),
	};
	assert!(
		parser.is_line_start(start),
		"{start} is not the start of a line"
	);
	let (nodes, _, _) = parser.blocks(start, parser.text.len());
	Parsed {
		nodes,
		diagnostics: parser.diagnostics,
	}
}

/// Parses a whole source as inline content, as in a paragraph: no block rule starts in it.
pub fn parse_inline(rules: &RuleSet, source: Arc<Source>) -> Parsed {
	let mut parser = Parser {
		rules,
		text: source.content(),
		source: &source,
		frames: Vec::new(),
		depth: 0,
		diagnostics: Vec::new(),
	};
	let (nodes, _, _) = parser.inline(0, parser.text.len());
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
	/// The nesting of the containers and frames around this source.
	depth: usize,
	diagnostics: Vec<Diagnostic>,
}

/// A delimiter, with captures resolved.
#[derive(Debug, Clone)]
struct Delimiter {
	anchored: bool,
	kind: Close,
}

#[derive(Debug, Clone)]
enum Close {
	Literal(String),
	LineEnd,
	/// A run of exactly this many times the character.
	Run(char, usize),
}

impl Close {
	fn text(&self) -> Option<String> {
		match self {
			Close::Literal(text) => Some(text.clone()),
			Close::LineEnd => None,
			Close::Run(c, n) => Some(c.to_string().repeat(*n)),
		}
	}
}

/// Resolves delimiter patterns against the captures of the current match. A `Same` whose
/// capture did not match (it was in a branch not taken) is left out.
fn resolve(patterns: &[DelimiterPattern], captures: &[Capture]) -> Vec<Delimiter> {
	patterns
		.iter()
		.filter_map(|pattern| {
			let kind = match &pattern.kind {
				DelimiterKind::Literal(text) => Close::Literal(text.clone()),
				DelimiterKind::LineEnd => Close::LineEnd,
				DelimiterKind::Same { name, run } => same(captures, name, *run)?,
			};
			Some(Delimiter {
				anchored: pattern.anchored,
				kind,
			})
		})
		.collect()
}

/// What `Same(name)` must match.
fn same(captures: &[Capture], name: &str, run: Option<char>) -> Option<Close> {
	let capture = captures.iter().rev().find(|c| &*c.name == name)?;
	let text = capture.token.content();
	Some(match run {
		Some(c) => Close::Run(c, text.chars().count()),
		None => Close::Literal(text.to_string()),
	})
}

/// The delimiters that end a content atom.
struct Frame {
	delimiters: Vec<Delimiter>,
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
	diagnostic: Box<Diagnostic>,
	/// Where the match stopped.
	pos: usize,
	/// The partial value of an unclosed atom, kept in the tree.
	value: Option<Value>,
}

impl Error {
	fn new(diagnostic: Diagnostic, pos: usize) -> Self {
		Self {
			diagnostic: Box::new(diagnostic),
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

	/// The length of an escape at `pos`: `\` and an ASCII punctuation character, which then
	/// neither opens nor closes anything.
	fn escape(&self, pos: usize, end: usize) -> Option<usize> {
		let rest = &self.text.as_bytes()[pos..end];
		(rest.len() >= 2 && rest[0] == b'\\' && rest[1].is_ascii_punctuation()).then_some(2)
	}

	/// The length of a comment at `pos`: `::` at the start of a line or after a blank, up to
	/// the end of the line. `foo::bar` is no comment.
	fn comment(&self, pos: usize, end: usize) -> Option<usize> {
		if !self.text[pos..end].starts_with("::") {
			return None;
		}
		let after_blank = self.text[..pos].ends_with([' ', '\t']);
		(self.is_line_start(pos) || after_blank).then(|| self.line_end(pos, end) - pos)
	}

	/// The frames of the current phase, innermost first: inline frames inside inline
	/// content, block frames between blocks.
	fn phase_frames(&self, inline: bool) -> Vec<(usize, &Frame)> {
		self.frames
			.iter()
			.enumerate()
			.rev()
			.take_while(|(_, frame)| frame.inline == inline)
			.collect()
	}

	/// The length of the longest delimiter of a frame at `pos`.
	fn frame_delimiter(&self, frame: &Frame, pos: usize, end: usize) -> Option<usize> {
		frame
			.delimiters
			.iter()
			.filter_map(|d| self.delimiter_at(d, pos, end))
			.max()
	}

	/// The length of a delimiter at `pos`.
	fn delimiter_at(&self, delimiter: &Delimiter, pos: usize, end: usize) -> Option<usize> {
		if delimiter.anchored && !self.is_line_start(pos) {
			return None;
		}
		self.close_at(&delimiter.kind, pos, end)
	}

	fn close_at(&self, close: &Close, pos: usize, end: usize) -> Option<usize> {
		match close {
			Close::Literal(text) => self.text[pos..end]
				.starts_with(text.as_str())
				.then_some(text.len()),
			Close::LineEnd => self.line_break(pos, end),
			Close::Run(c, n) => {
				let preceded = self.text[..pos].ends_with(*c);
				let (count, len) = self.run(*c, pos, end);
				(!preceded && count == *n).then_some(len)
			}
		}
	}

	/// The run of `c` at `pos`: its length in characters and in bytes.
	fn run(&self, c: char, pos: usize, end: usize) -> (usize, usize) {
		let count = self.text[pos..end].chars().take_while(|x| *x == c).count();
		(count, count * c.len_utf8())
	}

	/// The rule to dispatch at `pos`, or the frame that a delimiter there closes.
	///
	/// When the innermost frame's delimiter matches, the frames below it may close right
	/// after it: `***` closes `*` then `**`. Their delimiters form a chain, and the frame
	/// closes if the chain is at least as long as the longest opener. So inside `*…*`, `**`
	/// opens bold, while in `***x***`, the final `***` closes italic, then bold.
	///
	/// A delimiter of an outer frame alone means the inner frames are unclosed. An opener
	/// wins over it, so that `*c*` inside `**…**` inside `*…*` is italic.
	fn next(&self, table: Table, pos: usize, end: usize) -> Result<Option<(RuleId, usize)>, usize> {
		let opener = self.rules.dispatch(table, &self.text[pos..end]);
		let frames = self.phase_frames(table == Table::Inline);
		if let Some((innermost, frame)) = frames.first()
			&& let Some(len) = self.frame_delimiter(frame, pos, end)
		{
			let mut covered = len;
			for (_, outer) in &frames[1..] {
				match self.frame_delimiter(outer, pos + covered, end) {
					Some(len) if len > 0 => covered += len,
					_ => break,
				}
			}
			return match opener {
				Some((_, len)) if len > covered => Ok(opener),
				_ => Err(*innermost),
			};
		}
		if opener.is_some() {
			return Ok(opener);
		}
		for (index, frame) in frames.iter().skip(1) {
			if self.frame_delimiter(frame, pos, end).is_some() {
				return Err(*index);
			}
		}
		Ok(None)
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
			if let Some(len) = self.comment(pos, end) {
				nodes.push(Node::Comment(self.token(pos..pos + len)));
				pos = self.next_line(pos, end);
				continue;
			}
			let rule = match self.next_block(pos, end) {
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
	fn paragraph_end(&mut self, mut pos: usize, end: usize) -> (usize, usize) {
		loop {
			let content_end = self.line_end(pos, end);
			let next = self.next_line(pos, end);
			if next >= end
				|| self.is_blank_line(next, end)
				|| !matches!(self.next_block(next, end), Ok(None))
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
			if let Some(len) = self.escape(pos, end) {
				if text_start < pos {
					nodes.push(Node::Text(self.token(text_start..pos)));
				}
				nodes.push(Node::Escape(self.token(pos..pos + len)));
				pos += len;
				text_start = pos;
				continue;
			}
			if let Some(len) = self.comment(pos, end) {
				if text_start < pos {
					nodes.push(Node::Text(self.token(text_start..pos)));
				}
				nodes.push(Node::Comment(self.token(pos..pos + len)));
				// A comment that fills its line takes the line break with it.
				let line_start = self.is_line_start(pos);
				pos += len;
				if line_start && pos < end {
					pos = self.next_line(pos, end);
				}
				text_start = pos;
				continue;
			}
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

	/// The block rule to dispatch at `pos`, the start of a line, or the frame that a
	/// delimiter there closes. A rule whose guard does not match there is no rule.
	fn next_block(&mut self, pos: usize, end: usize) -> Result<Option<(RuleId, usize)>, usize> {
		let next = self.next(Table::Block, pos, end)?;
		Ok(next.filter(|(rule, len)| self.guard_matches(*rule, pos..pos + len, end)))
	}

	/// Whether the guard of a rule matches at `opener`, which leaves no trace.
	fn guard_matches(&mut self, rule: RuleId, opener: Range<usize>, end: usize) -> bool {
		let guard = self.rules.get(rule).unwrap().guard;
		if guard == 0 {
			return true;
		}
		let start = opener.start;
		let context = Context { rule, opener, end };
		let diagnostics = self.diagnostics.len();
		let frames = self.frames.len();
		let ops = &self.rules.ops(rule)[..guard];
		let matched = self.seq(&context, ops, start, &mut Vec::new()).is_ok();
		self.diagnostics.truncate(diagnostics);
		self.frames.truncate(frames);
		matched
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
				self.error(*error.diagnostic);
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
					.find(|ops| self.starts_seq(ops, pos, end, captures))
				{
					Some(ops) => Ok((self.seq(context, ops, pos, captures)?, Value::Span)),
					None => Err(self.expected(op, pos, end)),
				}
			}
			Op::Repeat { ops, min, max } => {
				let mut count = 0;
				let mut pos = pos;
				while max.is_none_or(|max| count < max) && self.starts_seq(ops, pos, end, captures)
				{
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
			Op::Blanks => match blanks(rest) {
				0 => Err(self.expected(op, pos, end)),
				len => matched(len),
			},
			Op::RestOfLine => Ok((self.line_end(pos, end), Value::Span)),
			Op::Text(delimiters) => self.text_until(context, &resolve(delimiters, captures), pos),
			Op::InlineContent(delimiters) => {
				let limit = self.before_blank_line(pos, end);
				let delimiters = resolve(delimiters, captures);
				self.content(context, delimiters, true, pos, limit)
			}
			Op::Content(delimiters) => {
				let delimiters = resolve(delimiters, captures);
				self.content(context, delimiters, false, pos, end)
			}
			Op::Options(schema) => self.options(schema, pos, end),
			Op::Container(prefix) => self.container(context, prefix, pos),
			Op::Block => self.block(context, pos),
			Op::Run { char, min } => match self.run(*char, pos, end) {
				(count, len) if count >= *min => matched(len),
				_ => Err(self.expected(op, pos, end)),
			},
			Op::Same { name, run } => {
				let close = same(captures, name, *run);
				match close
					.as_ref()
					.and_then(|close| self.close_at(close, pos, end))
				{
					Some(len) => matched(len),
					None => {
						let token = close.and_then(|close| close.text()).unwrap_or_default();
						let kind = DiagnosticKind::ExpectedToken { token };
						let location = self.char_token(pos, end);
						Err(Error::new(
							Diagnostic::new(kind, DiagnosticMessage::new(location)),
							pos,
						))
					}
				}
			}
			Op::Capture { name, class, op } => {
				let index = captures.len();
				let source = self.source;
				let capture = |after: usize, value| Capture {
					name: name.clone(),
					class: class.clone(),
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
	fn starts_seq(&self, ops: &[Op], pos: usize, end: usize, captures: &[Capture]) -> bool {
		match ops {
			[Op::LineStart, rest @ ..] if !rest.is_empty() => {
				self.is_line_start(pos) && self.starts_seq(rest, pos, end, captures)
			}
			[first, ..] => self.starts(first, pos, end, captures),
			[] => true,
		}
	}

	/// Whether `op` can start at `pos`.
	fn starts(&self, op: &Op, pos: usize, end: usize, captures: &[Capture]) -> bool {
		let rest = &self.text[pos..end];
		match op {
			Op::Literal(text) => rest.starts_with(text.as_str()),
			Op::LineStart => self.is_line_start(pos),
			Op::LineEnd => self.line_break(pos, end).is_some(),
			Op::Word => word(rest) > 0,
			Op::Blanks => blanks(rest) > 0,
			Op::Number { signed, decimal } => number(rest, *signed, *decimal).is_some(),
			Op::Capture { op, .. } => self.starts(op, pos, end, captures),
			Op::Or(alternatives) => alternatives
				.iter()
				.any(|ops| self.starts_seq(ops, pos, end, captures)),
			Op::Repeat { ops, .. } => self.starts_seq(ops, pos, end, captures),
			Op::Run { char, min } => self.run(*char, pos, end).0 >= *min,
			Op::Options(_) => rest.starts_with('['),
			Op::Container(_) | Op::Block => true,
			Op::Same { name, run } => same(captures, name, *run)
				.is_some_and(|close| self.close_at(&close, pos, end).is_some()),
			Op::Text(_) | Op::RestOfLine | Op::InlineContent(_) | Op::Content(_) => true,
		}
	}

	/// Verbatim text up to one of `delimiters`. Outer frames do not end it.
	fn text_until(
		&mut self,
		context: &Context,
		delimiters: &[Delimiter],
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
						diagnostic: Box::new(self.unclosed(context, delimiters, pos, end)),
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
		delimiters: Vec<Delimiter>,
		inline: bool,
		start: usize,
		end: usize,
	) -> Result<(usize, Value), Error> {
		self.check_nesting(context, start)?;
		self.frames.push(Frame { delimiters, inline });
		let own = self.frames.len() - 1;
		let (nodes, pos, stop) = if inline {
			self.inline(start, end)
		} else {
			self.blocks(start, end)
		};
		let delimiters = self.frames.pop().unwrap().delimiters;
		if stop == Stop::Frame(own) {
			Ok((pos, Value::Nodes(nodes)))
		} else {
			Err(Error {
				diagnostic: Box::new(self.unclosed(context, &delimiters, pos, end)),
				pos,
				value: Some(Value::Nodes(nodes)),
			})
		}
	}

	fn check_nesting(&self, context: &Context, pos: usize) -> Result<(), Error> {
		if self.depth + self.frames.len() < MAX_NESTING {
			return Ok(());
		}
		let kind = DiagnosticKind::NestingTooDeep { limit: MAX_NESTING };
		let location = self.token(context.opener.clone());
		Err(Error::new(
			Diagnostic::new(kind, DiagnosticMessage::new(location)),
			pos,
		))
	}

	/// One block from `start`, which must start a line: a match of a block rule, or a
	/// paragraph. Comment lines before it are part of it.
	fn block(&mut self, context: &Context, start: usize) -> Result<(usize, Value), Error> {
		self.check_nesting(context, start)?;
		let end = context.end;
		let mut nodes = Vec::new();
		let mut pos = start;
		while pos < end
			&& self.is_line_start(pos)
			&& let Some(len) = self.comment(pos, end)
		{
			nodes.push(Node::Comment(self.token(pos..pos + len)));
			pos = self.next_line(pos, end);
		}
		let missing = |this: &Self| Err(this.expected(&Op::Block, pos, end));
		if pos >= end || !self.is_line_start(pos) || self.is_blank_line(pos, end) {
			return missing(self);
		}
		let after = match self.next_block(pos, end) {
			// A delimiter of an enclosing construct, such as `:end`.
			Err(_) => return missing(self),
			Ok(Some((rule, len))) => {
				let m = self.rule(rule, pos..pos + len, end);
				let after = m.token.end();
				nodes.push(Node::Match(m));
				after
			}
			Ok(None) => {
				let (content_end, next) = self.paragraph_end(pos, end);
				let (children, _, _) = self.inline(pos, content_end);
				nodes.push(Node::Paragraph {
					token: self.token(pos..content_end),
					children,
				});
				next
			}
		};
		Ok((after, Value::Nodes(nodes)))
	}

	/// A container from `start`: the lines that belong to it, without their prefix, are
	/// parsed as blocks in a virtual source.
	fn container(
		&mut self,
		context: &Context,
		prefix: &Prefix,
		start: usize,
	) -> Result<(usize, Value), Error> {
		self.check_nesting(context, start)?;
		let end = context.end;
		// A list item's lines are indented to the end of its marker, its opener.
		let marker = context.opener.end;
		let line_start = self.text[..marker].rfind('\n').map_or(0, |i| i + 1);
		let indent = marker - line_start;

		let mut pieces = Vec::new();
		let mut pos = start;
		if !self.is_line_start(start) {
			pos = self.next_line(start, end);
			pieces.push(start..pos);
		}
		// Blank lines that belong to the container only if an indented line follows.
		let mut pending = Vec::new();
		while pos < end {
			let next = self.next_line(pos, end);
			let text = self.text;
			let line = &text[pos..self.line_end(pos, end)];
			let continues = *prefix == Prefix::Blanks
				&& line.starts_with([' ', '\t'])
				&& !self.is_blank_line(pos, end)
				&& matches!(self.next_block(pos, end), Ok(None));
			let content = match prefix {
				Prefix::Blanks if continues => pos + blanks(line)..next,
				Prefix::Literal(prefix) if line.starts_with(prefix.as_str()) => {
					pos + prefix.len()..next
				}
				Prefix::Literal(prefix) if line == prefix.trim_end() => pos + line.len()..next,
				Prefix::Indent if indent > 0 && self.is_blank_line(pos, end) => {
					let skip = line.len().min(indent);
					pending.push(pos + skip..next);
					pos = next;
					continue;
				}
				Prefix::Indent if indent > 0 && line.starts_with(&" ".repeat(indent)) => {
					pos + indent..next
				}
				_ => break,
			};
			pieces.append(&mut pending);
			pieces.push(content);
			pos = next;
		}
		// The match ends after the last line of the container, not after trailing blanks.
		let match_end = pieces.last().map_or(start, |piece| piece.end);

		let mut builder =
			VirtualSourceBuilder::new(self.source.clone(), self.container_location(start), start);
		for piece in pieces {
			builder.copy(piece);
		}
		let source = Arc::new(builder.build());
		let mut nested = Parser {
			rules: self.rules,
			text: source.content(),
			source: &source,
			frames: Vec::new(),
			depth: self.depth + self.frames.len() + 1,
			diagnostics: Vec::new(),
		};
		let (nodes, _, _) = nested.blocks(0, nested.text.len());
		self.diagnostics.append(&mut nested.diagnostics);
		Ok((match_end, Value::Nodes(nodes)))
	}

	/// `virtual:///<root path>/container/<root offset>`.
	fn container_location(&self, start: usize) -> Url {
		let (root, offset) = self.source.root_offset(start, Bias::Start);
		let mut location = Url::parse("virtual:///").unwrap();
		location.set_path(&format!("{}/container/{offset}", root.location().path()));
		location
	}

	/// The error for a construct whose delimiter is missing: reported at the opener, with the
	/// place where it had to end, and a fix that escapes the opener.
	fn unclosed(
		&self,
		context: &Context,
		delimiters: &[Delimiter],
		pos: usize,
		end: usize,
	) -> Diagnostic {
		let rule = &self.rules.get(context.rule).unwrap().name;
		let token = delimiters
			.iter()
			.find_map(|d| d.kind.text())
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

	/// An option list at `start`. Syntax errors fail the rule; an option that does not fit
	/// the schema is reported, and the match goes on.
	///
	/// Blanks and line breaks are skipped around `[`, `=`, `,` and `]`, so a list can span
	/// several lines, but a key and a value each stay on one line. A trailing `,` is allowed.
	fn options(
		&mut self,
		schema: &OptionSchema,
		start: usize,
		end: usize,
	) -> Result<(usize, Value), Error> {
		let text = &self.text[..end];
		let skip = |pos: usize| {
			end - text[pos..]
				.trim_start_matches([' ', '\t', '\r', '\n'])
				.len()
		};
		let fail = |this: &Self, kind, pos| {
			let location = this.char_token(pos, end);
			Err(Error::new(
				Diagnostic::new(kind, DiagnosticMessage::new(location)),
				pos,
			))
		};
		let expected_token = |token: &str| DiagnosticKind::ExpectedToken {
			token: token.into(),
		};

		if !text[start..].starts_with('[') {
			return fail(self, expected_token("["), start);
		}
		let mut pos = skip(start + 1);
		let mut entries = Vec::new();
		while !text[pos..].starts_with(']') {
			let len = word(&text[pos..]);
			if len == 0 {
				let atom = "Word".into();
				return fail(self, DiagnosticKind::ExpectedAtom { atom }, pos);
			}
			let key = self.token(pos..pos + len);
			pos = skip(pos + len);
			let mut entry = OptionEntry {
				key,
				value_token: None,
				value: None,
			};
			if text[pos..].starts_with('=') {
				let value_start = skip(pos + 1);
				let (value_end, value) = self.option_value(value_start, end)?;
				entry.value_token = Some(self.token(value_start..value_end));
				entry.value = Some(value);
				pos = skip(value_end);
			}
			entries.push(entry);
			match text[pos..].chars().next() {
				Some(',') => pos = skip(pos + 1),
				Some(']') => {}
				_ => return fail(self, expected_token("]"), pos),
			}
		}
		pos += 1;

		let values = self.check_options(schema, &entries, start..pos);
		Ok((pos, Value::Options(Options { entries, values })))
	}

	/// A value in an option list: its end and its text without quotes and escapes.
	fn option_value(&self, start: usize, end: usize) -> Result<(usize, String), Error> {
		let line_end = self.line_end(start, end);
		let text = &self.text[..line_end];
		let mut value = String::new();
		let mut chars = text[start..].char_indices().map(|(i, c)| (start + i, c));
		if text[start..].starts_with('"') {
			chars.next();
			while let Some((i, c)) = chars.next() {
				match c {
					'"' => return Ok((i + 1, value)),
					'\\' => match chars.next() {
						Some((_, escaped)) => value.push(escaped),
						None => break,
					},
					c => value.push(c),
				}
			}
			let kind = DiagnosticKind::ExpectedToken { token: "\"".into() };
			let location = self.char_token(line_end, end);
			return Err(Error::new(
				Diagnostic::new(kind, DiagnosticMessage::new(location)),
				line_end,
			));
		}

		// A bare value ends at `,` or `]`, without trailing blanks.
		let mut value_end = start;
		let mut trimmed = 0;
		while let Some((i, c)) = chars.next() {
			match c {
				',' | ']' => break,
				'\\' if text[i + 1..].starts_with(|c: char| c.is_ascii_punctuation()) => {
					let (j, escaped) = chars.next().unwrap();
					value.push(escaped);
					value_end = j + 1;
					trimmed = value.len();
				}
				c => {
					value.push(c);
					if c != ' ' && c != '\t' {
						value_end = i + c.len_utf8();
						trimmed = value.len();
					}
				}
			}
		}
		value.truncate(trimmed);
		Ok((value_end, value))
	}

	/// Checks entries against the schema, and returns the value of every option.
	fn check_options(
		&mut self,
		schema: &OptionSchema,
		entries: &[OptionEntry],
		list: Range<usize>,
	) -> Vec<(String, OptionValue)> {
		let mut given: Vec<(&str, OptionValue)> = Vec::new();
		for (index, entry) in entries.iter().enumerate() {
			let name = entry.key.content();
			let report = |kind| Diagnostic::new(kind, DiagnosticMessage::new(entry.key.clone()));
			if let Some(first) = entries[..index]
				.iter()
				.find(|other| other.key.content() == name)
			{
				let kind = DiagnosticKind::DuplicateOption { name: name.into() };
				let diagnostic =
					report(kind).with(DiagnosticMessage::fluent(first.key.clone(), "first"));
				self.error(diagnostic);
				continue;
			}
			let Some(spec) = schema.get(name) else {
				self.error(report(DiagnosticKind::UnknownOption { name: name.into() }));
				continue;
			};
			match spec.kind.convert(entry.value.as_deref()) {
				Some(value) => given.push((name, value)),
				None => {
					let kind = DiagnosticKind::InvalidOptionValue {
						name: name.into(),
						expected: spec.kind.to_string(),
					};
					let location = entry.value_token.clone().unwrap_or(entry.key.clone());
					self.error(Diagnostic::new(kind, DiagnosticMessage::new(location)));
				}
			}
		}

		let mut values = Vec::new();
		for spec in &schema.options {
			let value = given
				.iter()
				.find(|(name, _)| *name == spec.name)
				.map(|(_, value)| value.clone())
				.or_else(|| spec.default.clone());
			match value {
				Some(value) => values.push((spec.name.clone(), value)),
				// An invalid value was already reported.
				None if entries.iter().any(|e| e.key.content() == spec.name) => {}
				None => {
					let kind = DiagnosticKind::MissingOption {
						name: spec.name.clone(),
					};
					let location = self.token(list.clone());
					self.error(Diagnostic::new(kind, DiagnosticMessage::new(location)));
				}
			}
		}
		values
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
/// The length of the spaces and tabs at the start of `text`.
fn blanks(text: &str) -> usize {
	text.len() - text.trim_start_matches([' ', '\t']).len()
}

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
pub(crate) mod tests {
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
				Node::Escape(token) => format!("E({})", &token.content()[1..]),
				Node::Comment(token) => format!("C({})", token.content()),
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
								Value::Options(options) => {
									let values: Vec<_> = options
										.values
										.iter()
										.map(|(name, value)| {
											let value = match value {
												OptionValue::Text(text) => format!("{text:?}"),
												OptionValue::Integer(n) => n.to_string(),
												OptionValue::Number(n) => n.to_string(),
												OptionValue::Bool(b) => b.to_string(),
											};
											format!("{name}:{value}")
										})
										.collect();
									format!("{{{}}}", values.join(" "))
								}
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

	pub(crate) fn rules() -> RuleSet {
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
				vec![
					Atom::capture("fence", Atom::run('`', 1)).with_class("operator"),
					Atom::capture("code", Atom::Text).with_class("string"),
					Atom::same("fence"),
				],
			),
			rule(
				"fence",
				vec![
					Atom::LineStart,
					Atom::capture("fence", Atom::run('`', 3)),
					Atom::capture("info", Atom::RestOfLine),
					Atom::LineEnd,
					Atom::capture("body", Atom::Text),
					Atom::LineStart,
					Atom::same("fence"),
					Atom::LineEnd,
				],
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

	pub(crate) fn check(text: &str, expected: &str) -> Vec<Diagnostic> {
		let rules = rules();
		let parsed = parse(&rules, source(text));
		assert_eq!(show(&rules, &parsed.nodes), expected);
		parsed.diagnostics
	}

	#[test]
	fn parse_from_a_line() {
		let rules = rules();
		let parsed = parse_from(&rules, source("**a\n\n# B\nc"), 5);
		assert_eq!(
			show(&rules, &parsed.nodes),
			r#"heading(deeper="" title="B") P["c"]"#
		);
		assert!(parsed.diagnostics.is_empty());
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
	fn comments() {
		// After a blank or at the start of a line; `a::b` is text.
		let diagnostics = check(
			"a::b c :: note `x`\n:: whole line\nd\tdone\t::x\n\n:: alone\n\ne \\:: f",
			r#"P["a::b c " C(:: note `x`) "\n" C(:: whole line) "d\tdone\t" C(::x)] C(:: alone) P["e " E(:) ": f"]"#,
		);
		assert_diagnostics!(diagnostics, []);

		// A comment ends unclosed constructs.
		let diagnostics = check("*a :: b*", r#"P[italic!(body=["a " C(:: b*)])]"#);
		assert_diagnostics!(diagnostics, [1001 @ 0..1]);

		// Verbatim text has no comments.
		let diagnostics = check("`a :: b`", r#"P[code(fence="`" code="a :: b")]"#);
		assert_diagnostics!(diagnostics, []);
	}

	#[test]
	fn optional_parts_do_not_delimit_themselves() {
		// An optional part matches once: its first atom (`" "`) does not end the content
		// inside it.
		let mut rules = RuleSet::new();
		let rule = Rule::new(
			"tag",
			vec![
				lit("<"),
				Atom::optional(vec![lit(" "), Atom::capture("title", Atom::InlineContent)]),
				lit(">"),
			],
		);
		rules.add(rule).unwrap();
		let parsed = parse(&rules, source("< a b c>"));
		assert_diagnostics!(parsed.diagnostics, []);
		assert_eq!(show(&rules, &parsed.nodes), r#"P[tag(title=["a b c"])]"#);
	}

	#[test]
	fn inline_sources() {
		let rules = rules();
		let parsed = parse_inline(&rules, source("# *a*\n- b"));
		assert_diagnostics!(parsed.diagnostics, []);
		assert_eq!(
			show(&rules, &parsed.nodes),
			r##""# " italic(body=["a"]) "\n- b""##
		);
		let parsed = parse_inline(&rules, source("*a"));
		assert_diagnostics!(parsed.diagnostics, [1001 @ 0..1]);
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
			r#"P["a " code(fence="`" code="b") " " link(url="x" title="y") " " link(url="z") " " list(item="a" item="b" item="c") " é"]"#,
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
		let diagnostics = check(
			"a {} `b`",
			r#"P["a " list!() "} " code(fence="`" code="b")]"#,
		);
		assert_diagnostics!(diagnostics, [1003 @ 3..4]);
	}

	#[test]
	fn unclosed_text() {
		let rules = rules();
		let parsed = parse(&rules, source("a `b\n\nc"));
		assert_eq!(
			show(&rules, &parsed.nodes),
			r#"P["a " code!(fence="`" code="b")] P["c"]"#
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
			r#"P[italic(body=["a " bold(body=["b"]) " c"]) " " bold(body=["a " italic(body=["b"]) " c"]) " " bold(body=[code(fence="`" code="a**b")])]"#,
		);
		assert_diagnostics!(diagnostics, []);
	}

	#[test]
	fn closing_chains() {
		for (text, expected) in [
			("***x***", r#"P[bold(body=[italic(body=["x"])])]"#),
			("**a *b***", r#"P[bold(body=["a " italic(body=["b"])])]"#),
			("*a **b***", r#"P[italic(body=["a " bold(body=["b"])])]"#),
			(
				"*a **b** c*",
				r#"P[italic(body=["a " bold(body=["b"]) " c"])]"#,
			),
			(
				"*a **b *c* d** e*",
				r#"P[italic(body=["a " bold(body=["b " italic(body=["c"]) " d"]) " e"])]"#,
			),
		] {
			let diagnostics = check(text, expected);
			assert_diagnostics!(diagnostics, []);
		}
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

	#[test]
	fn runs() {
		let diagnostics = check(
			"``a`b`` `a``b` x",
			r#"P[code(fence="``" code="a`b") " " code(fence="`" code="a``b") " x"]"#,
		);
		assert_diagnostics!(diagnostics, []);

		let diagnostics = check(
			"```rust\nfn x() {}\n````\n```\nafter",
			r#"fence(fence="```" info="rust" body="fn x() {}\n````\n") P["after"]"#,
		);
		assert_diagnostics!(diagnostics, []);

		let diagnostics = check("```\ncode", r#"fence!(fence="```" info="" body="code")"#);
		assert_diagnostics!(diagnostics, [1001 @ 0..3]);
	}

	#[test]
	fn capture_classes() {
		let rules = rules();
		let parsed = parse(&rules, source("`x`"));
		let Node::Paragraph { children, .. } = &parsed.nodes[0] else {
			panic!()
		};
		let Node::Match(m) = &children[0] else {
			panic!()
		};
		assert_eq!(
			m.capture("fence").unwrap().class.as_deref(),
			Some("operator")
		);
		assert_eq!(m.capture("code").unwrap().class.as_deref(), Some("string"));
	}

	pub(crate) fn graph_rules() -> RuleSet {
		use crate::OptionKind;
		use crate::OptionSpec;

		let schema = OptionSchema::new([
			OptionSpec::new(
				"layout",
				OptionKind::Choice(vec!["dot".into(), "neato".into()]),
			)
			.with_default(OptionValue::Text("dot".into())),
			OptionSpec::new("width", OptionKind::Integer).with_default(OptionValue::Integer(100)),
			OptionSpec::new("scale", OptionKind::Number),
			OptionSpec::new("caption", OptionKind::Text).with_default(OptionValue::Text("".into())),
			OptionSpec::new("inline", OptionKind::Bool).with_default(OptionValue::Bool(false)),
		]);
		let mut rules = RuleSet::new();
		rules
			.add(Rule::new(
				"graph",
				vec![
					Atom::LineStart,
					lit(":graph"),
					Atom::optional(vec![Atom::capture(
						"options",
						Atom::Options(Arc::new(schema)),
					)]),
					Atom::LineEnd,
				],
			))
			.unwrap();
		rules
	}

	pub(crate) fn check_graph(text: &str, expected: &str) -> Vec<Diagnostic> {
		let rules = graph_rules();
		let parsed = parse(&rules, source(text));
		assert_eq!(show(&rules, &parsed.nodes), expected);
		parsed.diagnostics
	}

	#[test]
	fn options() {
		let diagnostics = check_graph(
			r#":graph[layout=neato, scale=1.5, caption="a, \"b\"", inline]"#,
			r#"graph(options={layout:"neato" width:100 scale:1.5 caption:"a, \"b\"" inline:true})"#,
		);
		assert_diagnostics!(diagnostics, []);

		let diagnostics = check_graph(
			r#":graph[ scale = 2 , caption=a\, b\] ,width=-3]"#,
			r#"graph(options={layout:"dot" width:-3 scale:2 caption:"a, b]" inline:false})"#,
		);
		assert_diagnostics!(diagnostics, []);

		let rules = graph_rules();
		let parsed = parse(&rules, source(":graph[caption=a  b  ,scale=1]"));
		let Node::Match(m) = &parsed.nodes[0] else {
			panic!()
		};
		let Value::Options(options) = &m.capture("options").unwrap().value else {
			panic!()
		};
		assert_eq!(options.entries[0].value.as_deref(), Some("a  b"));
		assert_eq!(
			options.entries[0].value_token.as_ref().unwrap().range(),
			15..19
		);
		assert_eq!(options.get("scale"), Some(&OptionValue::Number(1.0)));
	}

	#[test]
	fn options_on_several_lines() {
		let diagnostics = check_graph(
			":graph[\n\tlayout = neato,\n\tscale =\n\t\t2,\n\tcaption = a b\n\t, inline,\n]\n",
			r#"graph(options={layout:"neato" width:100 scale:2 caption:"a b" inline:true})"#,
		);
		assert_diagnostics!(diagnostics, []);
	}

	#[test]
	fn invalid_options() {
		let text = ":graph[nope=1, scale=x, scale=2, layout=fdp, inline=yes]";
		let diagnostics = check_graph(
			text,
			r#"graph(options={layout:"dot" width:100 caption:"" inline:false})"#,
		);
		assert_diagnostics!(diagnostics, [1005 @ 7..11, 1007 @ 21..22, 1006 @ 24..29, 1007 @ 40..43, 1007 @ 52..55]);
		assert_eq!(
			diagnostics[3].kind,
			DiagnosticKind::InvalidOptionValue {
				name: "layout".into(),
				expected: "dot, neato".into()
			}
		);

		let diagnostics = check_graph(
			":graph[]\n:graph",
			r#"graph(options={layout:"dot" width:100 caption:"" inline:false}) graph()"#,
		);
		assert_diagnostics!(diagnostics, [1008 @ 6..8]);
	}

	#[test]
	fn option_syntax_errors() {
		for (text, id, range) in [
			(":graph[scale=1", 1001, 14..14),
			(":graph[scale=1\nx]", 1001, 15..16),
			(":graph[=1]", 1003, 7..8),
			(r#":graph[scale="1" x]"#, 1001, 17..18),
			(r#":graph[caption="x]"#, 1001, 18..18),
			(":graph[caption=\"x\n\"]", 1001, 17..18),
			(":graph[,]", 1003, 7..8),
		] {
			let rules = graph_rules();
			let parsed = parse(&rules, source(text));
			nml_diagnostics::testing::check(&parsed.diagnostics, &[(id, Some(range))]);
		}
	}

	#[test]
	fn escapes() {
		let diagnostics = check(
			r"a \*b\* \\ \q \",
			r#"P["a " E(*) "b" E(*) " " E(\) " \\q \\"]"#,
		);
		assert_diagnostics!(diagnostics, []);

		let diagnostics = check(
			r"*a \* b* `a\` \# x",
			r#"P[italic(body=["a " E(*) " b"]) " " code(fence="`" code="a\\") " " E(#) " x"]"#,
		);
		assert_diagnostics!(diagnostics, []);

		// An escaped character does not start a block either.
		let diagnostics = check("a\n\\# x\n\\:end", r#"P["a\n" E(#) " x\n" E(:) "end"]"#);
		assert_diagnostics!(diagnostics, []);
	}

	fn container_rules() -> RuleSet {
		let mut rules = rules();
		let quote = || Atom::Container(Prefix::Literal("> ".into()));
		for rule in [
			Rule::new(
				"quote",
				vec![Atom::LineStart, lit("> "), Atom::capture("body", quote())],
			),
			Rule::new(
				"callout",
				vec![
					Atom::LineStart,
					lit("> [!"),
					Atom::capture("kind", Atom::Word),
					lit("]"),
					Atom::LineEnd,
					Atom::capture("body", quote()),
				],
			),
			Rule::new(
				"item",
				vec![
					Atom::LineStart,
					lit("- "),
					Atom::capture("body", Atom::Container(Prefix::Indent)),
				],
			),
			Rule::new(
				"numbered",
				vec![
					Atom::LineStart,
					Atom::capture(
						"number",
						Atom::Number {
							signed: false,
							decimal: false,
						},
					),
					lit(". "),
					Atom::optional(vec![Atom::capture(
						"check",
						Atom::or([vec![lit("[ ] ")], vec![lit("[x] ")]]),
					)]),
					Atom::capture("body", Atom::Container(Prefix::Indent)),
				],
			),
		] {
			rules.add(rule).unwrap();
		}
		rules
	}

	fn check_containers(text: &str, expected: &str) -> Vec<Diagnostic> {
		let rules = container_rules();
		let parsed = parse(&rules, source(text));
		assert_eq!(show(&rules, &parsed.nodes), expected);
		parsed.diagnostics
	}

	#[test]
	fn containers() {
		let diagnostics = check_containers(
			"> a\n> **b**\n>\n> c\nd",
			r#"quote(body=[P["a\n" bold(body=["b"])] P["c"]]) P["d"]"#,
		);
		assert_diagnostics!(diagnostics, []);

		let diagnostics = check_containers(
			"> > x\n> > y\n> z\n",
			r#"quote(body=[quote(body=[P["x\ny"]]) P["z"]])"#,
		);
		assert_diagnostics!(diagnostics, []);

		let diagnostics = check_containers(
			"> [!note]\n> text\n\nafter",
			r#"callout(kind="note" body=[P["text"]]) P["after"]"#,
		);
		assert_diagnostics!(diagnostics, []);
	}

	#[test]
	fn indented_containers() {
		let diagnostics = check_containers(
			"- a\n  b\n\n  - c\n\n    d\n- e\n\nf",
			r#"item(body=[P["a\nb"] item(body=[P["c"] P["d"]])]) item(body=[P["e"]]) P["f"]"#,
		);
		assert_diagnostics!(diagnostics, []);
	}

	#[test]
	fn numbered_containers_indent_to_the_marker() {
		// The body is indented to the end of `10. `, not past the checkbox.
		let diagnostics = check_containers(
			"10. [x] a\n    b\n9. c\n2024 was\n1.5",
			r#"numbered(number=10 check="[x] " body=[P["a\nb"]]) numbered(number=9 body=[P["c"]]) P["2024 was\n1.5"]"#,
		);
		assert_diagnostics!(diagnostics, []);
	}

	#[test]
	fn single_blocks() {
		let mut rules = container_rules();
		let figure = Rule::new(
			"figure",
			vec![
				Atom::LineStart,
				lit(":fig"),
				Atom::LineEnd,
				Atom::capture("body", Atom::Block),
			],
		);
		rules.add(figure).unwrap();
		let check = |text: &str, expected: &str| {
			let parsed = parse(&rules, source(text));
			assert_eq!(show(&rules, &parsed.nodes), expected);
			parsed.diagnostics
		};
		// The block is a paragraph, up to its end.
		let diagnostics = check(":fig\na\nb\n\nc", r#"figure(body=[P["a\nb"]]) P["c"]"#);
		assert_diagnostics!(diagnostics, []);
		// Or a match of a block rule, after comment lines.
		let diagnostics = check(
			":fig\n:: note\n- a\n  b\nc",
			r#"figure(body=[C(:: note) item(body=[P["a\nb"]])]) P["c"]"#,
		);
		assert_diagnostics!(diagnostics, []);
		// Figures nest.
		let diagnostics = check(":fig\n:fig\nx", r#"figure(body=[figure(body=[P["x"]])])"#);
		assert_diagnostics!(diagnostics, []);

		for (missing, expected) in [
			(":fig", "figure!()"),
			(":fig\n", "figure!()"),
			(":fig\n\nx", r#"figure!() P["x"]"#),
		] {
			let diagnostics = check(missing, expected);
			assert_diagnostics!(diagnostics, [1003]);
		}
	}

	#[test]
	fn guarded_entries_with_continuations() {
		let mut rules = rules();
		let markers = || Atom::or([vec![lit("*")], vec![lit("-")]]);
		let entry = Rule::new(
			"entry",
			vec![
				Atom::LineStart,
				Atom::Blanks,
				Atom::capture("first", lit("*")),
				Atom::capture("more", Atom::repeat(vec![markers()], 0, None)),
				Atom::Blanks,
				Atom::capture("body", Atom::Container(Prefix::Blanks)),
			],
		)
		.guarded(5);
		rules.add(entry).unwrap();
		let check = |text: &str, expected: &str| {
			let parsed = parse(&rules, source(text));
			assert_eq!(show(&rules, &parsed.nodes), expected);
			parsed.diagnostics
		};
		// Continuation lines start with a blank; the next entry or a blank line ends one.
		let diagnostics = check(
			" * a\n   b\n\t*- c\n d\n\ne",
			r#"entry(first="*" more="" body=[P["a\nb"]]) entry(first="*" more="-" body=[P["c\nd"]]) P["e"]"#,
		);
		assert_diagnostics!(diagnostics, []);
		// Where the guard does not match, the opener is text: no blank after the markers.
		let diagnostics = check(
			" *word* and\n *x",
			r#"P[" " italic(body=["word"]) " and\n " italic!(body=["x"])]"#,
		);
		assert_diagnostics!(diagnostics, [1001]);
		let diagnostics = check("a\n *b*", r#"P["a\n " italic(body=["b"])]"#);
		assert_diagnostics!(diagnostics, []);
	}

	#[test]
	fn container_positions_map_to_the_file() {
		let rules = container_rules();
		let parsed = parse(&rules, source("> a\n> **b"));
		assert_diagnostics!(parsed.diagnostics, [1001 @ 2..4]);
		let primary = &parsed.diagnostics[0].primary().location;
		assert_eq!(
			primary.source().location().as_str(),
			"virtual:///test.nml/container/2"
		);
		assert_eq!(primary.root().range(), 6..8);
		assert_eq!(
			primary.root().source().location().as_str(),
			"file:///test.nml"
		);
	}

	#[test]
	fn container_nesting_is_bounded() {
		let rules = container_rules();
		let parsed = parse(&rules, source(&"> ".repeat(1000)));
		assert_diagnostics!(parsed.diagnostics, [1004]);
	}
}
