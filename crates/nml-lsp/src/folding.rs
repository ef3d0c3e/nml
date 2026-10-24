//! `textDocument/foldingRange`: heading sections, blocks, lists and comments.

use lsp_types::FoldingRange;
use lsp_types::FoldingRangeKind;
use nml_grammar::Node;
use nml_grammar::Value;
use nml_source::Token;
use nml_source::Url;

use crate::Result;
use crate::Server;
use crate::convert;

/// The lines of a token in its file, first and last: a token that ends with its line's
/// break ends on that line.
fn lines(token: &Token) -> (u32, u32) {
	let range = convert::range(token);
	let end = match range.end.character == 0 && range.end.line > range.start.line {
		true => range.end.line - 1,
		false => range.end.line,
	};
	(range.start.line, end)
}

/// Whether a token starts its line, after blanks: a block, not inline markup.
fn starts_line(token: &Token) -> bool {
	let root = token.root();
	let content = root.source().content();
	let before = &content[..root.start()];
	let line = &before[before.rfind('\n').map_or(0, |at| at + 1)..];
	line.chars().all(|c| c == ' ' || c == '\t')
}

fn range(start: u32, end: u32, kind: Option<FoldingRangeKind>) -> FoldingRange {
	FoldingRange {
		start_line: start,
		start_character: None,
		end_line: end,
		end_character: None,
		kind,
		collapsed_text: None,
	}
}

/// What folds in a tree.
struct Folds<'a> {
	language: &'a nml_lang::Language,
	file: &'a Url,
	out: Vec<FoldingRange>,
	/// The headings, by line, with their level: their sections fold.
	headings: Vec<(u32, usize)>,
	/// The run of lines being collected, of list entries or comments, and its kind.
	run: Option<(u32, u32, &'static str)>,
}

impl Folds<'_> {
	fn in_file(&self, token: &Token) -> bool {
		token.root().source().location() == self.file
	}

	/// Extends the current run of `kind` with the lines of a token, or starts one.
	fn extend(&mut self, kind: &'static str, (start, end): (u32, u32)) {
		match &mut self.run {
			Some((_, last, run_kind)) if *run_kind == kind && start <= *last + 1 => {
				*last = (*last).max(end);
			}
			_ => {
				self.flush();
				self.run = Some((start, end, kind));
			}
		}
	}

	fn flush(&mut self) {
		if let Some((start, end, kind)) = self.run.take()
			&& end > start
		{
			let fold_kind = (kind == "comment").then_some(FoldingRangeKind::Comment);
			self.out.push(range(start, end, fold_kind));
		}
	}

	fn nodes(&mut self, nodes: &[Node]) {
		for node in nodes {
			match node {
				Node::Comment(token) if self.in_file(token) => {
					self.extend("comment", lines(token));
				}
				Node::Text(token) if token.content().trim().is_empty() => {}
				Node::Paragraph { children, .. } => {
					self.flush();
					self.nodes(children);
				}
				Node::Match(m) if self.in_file(&m.token) => {
					let name = (self.language.rules().get(m.rule)).map(|rule| rule.name.as_str());
					match name {
						Some("heading") => {
							self.flush();
							let level = m.capture("level").map_or(1, |c| c.token.content().len());
							self.headings.push((lines(&m.token).0, level));
						}
						Some("bullet_entry" | "numbered_entry") => {
							self.extend("list", lines(&m.token));
						}
						_ => {
							self.flush();
							let (start, end) = lines(&m.token);
							if end > start && starts_line(&m.token) {
								self.out.push(range(start, end, None));
							}
						}
					}
					for capture in &m.captures {
						if let Value::Nodes(nodes) = &capture.value {
							let run = self.run.take();
							self.nodes(nodes);
							self.flush();
							self.run = run;
						}
					}
				}
				_ => self.flush(),
			}
		}
	}

	/// The sections of the headings: from a heading to the last line with content before
	/// the next heading of its level or above, or the end.
	fn sections(&mut self, source: &str) {
		let content_lines: Vec<bool> = source.lines().map(|line| !line.trim().is_empty()).collect();
		let last_content = |before: usize| -> u32 {
			(0..before)
				.rev()
				.find(|line| content_lines[*line])
				.unwrap_or(0) as u32
		};
		for (index, (line, level)) in self.headings.iter().enumerate() {
			let next = self.headings[index + 1..]
				.iter()
				.find(|(_, other)| other <= level)
				.map_or(content_lines.len(), |(next, _)| *next as usize);
			let end = last_content(next);
			if end > *line {
				self.out
					.push(range(*line, end, Some(FoldingRangeKind::Region)));
			}
		}
	}
}

impl Server {
	pub(crate) fn folding_ranges(&self, url: &Url) -> Result<Vec<FoldingRange>> {
		let Some(analysis) = self.analyses.get(url) else {
			return Ok(Vec::new());
		};
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let mut folds = Folds {
			language,
			file: url,
			out: Vec::new(),
			headings: Vec::new(),
			run: None,
		};
		folds.nodes(&analysis.tree);
		folds.flush();
		folds.sections(analysis.source.content());
		let mut out = folds.out;
		out.sort_by_key(|fold| (fold.start_line, std::cmp::Reverse(fold.end_line)));
		Ok(out)
	}
}
