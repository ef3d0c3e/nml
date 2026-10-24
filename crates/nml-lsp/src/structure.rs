//! Editing the outline: promoting, demoting and moving a heading's section, as org-mode
//! does. The commands take `[uri, line]`: the section of the heading on that line, or of the
//! last heading above it. The edit is sent to the client with `workspace/applyEdit`.

use std::collections::HashMap;
use std::ops::Range;
use std::str::FromStr;

use lsp_types::CodeAction;
use lsp_types::CodeActionKind;
use lsp_types::CodeActionOrCommand;
use lsp_types::Command;
use lsp_types::TextEdit;
use lsp_types::Uri;
use lsp_types::WorkspaceEdit;
use nml_grammar::Node;
use nml_grammar::Value;
use nml_source::Encoding;
use nml_source::Token;
use nml_source::Url;

use crate::Analysis;
use crate::Result;
use crate::Server;

pub(crate) const PROMOTE_COMMAND: &str = "nml.promote";
pub(crate) const DEMOTE_COMMAND: &str = "nml.demote";
pub(crate) const MOVE_UP_COMMAND: &str = "nml.moveUp";
pub(crate) const MOVE_DOWN_COMMAND: &str = "nml.moveDown";

pub(crate) const COMMANDS: [&str; 4] = [
	PROMOTE_COMMAND,
	DEMOTE_COMMAND,
	MOVE_UP_COMMAND,
	MOVE_DOWN_COMMAND,
];

/// A heading of the document.
#[derive(Debug, Clone)]
struct Heading {
	/// The offset of its line's start.
	line_start: usize,
	level: usize,
	/// Its `#` run.
	marker: Range<usize>,
}

/// The headings of a tree written in the file, in order.
fn headings(nodes: &[Node], language: &nml_lang::Language, file: &Url, out: &mut Vec<Heading>) {
	for node in nodes {
		let Node::Match(m) = node else {
			if let Node::Paragraph { children, .. } = node {
				headings(children, language, file, out);
			}
			continue;
		};
		let name = language.rules().get(m.rule).map(|rule| rule.name.as_str());
		if name == Some("heading")
			&& let Some(level) = m.capture("level")
			&& level.token.root().source().location() == file
		{
			let root: Token = level.token.root();
			let content = root.source().content();
			let line_start = content[..root.start()].rfind('\n').map_or(0, |at| at + 1);
			out.push(Heading {
				line_start,
				level: root.content().len(),
				marker: root.range(),
			});
		}
		for capture in &m.captures {
			if let Value::Nodes(nodes) = &capture.value {
				headings(nodes, language, file, out);
			}
		}
	}
}

/// The end of the section of `headings[index]`: the start of the next heading of its level
/// or above, or the end of the text.
fn section_end(headings: &[Heading], index: usize, text: &str) -> usize {
	let level = headings[index].level;
	(headings[index + 1..].iter())
		.find(|other| other.level <= level)
		.map_or(text.len(), |other| other.line_start)
}

/// The end of the content of `text[start..end]`: after its last line that is not blank.
fn content_end(text: &str, start: usize, end: usize) -> usize {
	let mut at = end;
	while at > start {
		let line_start = text[start..at - 1]
			.rfind('\n')
			.map_or(start, |i| start + i + 1);
		if !text[line_start..at].trim().is_empty() {
			return at;
		}
		at = line_start;
	}
	start
}

/// `text[start..end]` with its sections `start..middle` and `middle..end` swapped. The blank
/// lines after each section stay where they are, between the sections.
fn swap(text: &str, start: usize, middle: usize, end: usize) -> String {
	let first_end = content_end(text, start, middle);
	let second_end = content_end(text, middle, end);
	// The first section ends with a line break, since the second starts a line; the second
	// may end the text without one.
	let mut second = text[middle..second_end].to_string();
	if !second.ends_with('\n') {
		second.push('\n');
	}
	let mut out = format!(
		"{second}{}{}{}",
		&text[first_end..middle],
		&text[start..first_end],
		&text[second_end..end]
	);
	if !text[start..end].ends_with('\n') {
		out.pop();
	}
	out
}

impl Server {
	/// The headings of an open document, and the index of the one whose section holds
	/// `line`.
	fn section_at(&self, url: &Url, line: u64) -> Result<(&Analysis, Vec<Heading>, usize)> {
		let analysis = (self.analyses.get(url)).ok_or_else(|| format!("`{url}` is not open"))?;
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let mut found = Vec::new();
		headings(&analysis.tree, language, url, &mut found);
		let position = nml_source::Position::new(line as usize, 0);
		let offset = (analysis.source.offset(position, Encoding::Utf16))
			.ok_or("the line is not in the document")?;
		let index = (found.iter())
			.rposition(|heading| heading.line_start <= offset)
			.ok_or("no heading at or above the line")?;
		Ok((analysis, found, index))
	}

	/// The edits of a structure command, as `(range, replacement)` in bytes.
	fn structure_edits(
		&self,
		command: &str,
		url: &Url,
		line: u64,
	) -> Result<Vec<(Range<usize>, String)>> {
		let (analysis, headings, index) = self.section_at(url, line)?;
		let text = analysis.source.content();
		let end = section_end(&headings, index, text);
		let heading = &headings[index];
		let inside = || {
			(headings.iter())
				.filter(|other| other.line_start >= heading.line_start && other.line_start < end)
		};
		Ok(match command {
			PROMOTE_COMMAND | DEMOTE_COMMAND => {
				let delta: isize = if command == PROMOTE_COMMAND { -1 } else { 1 };
				let mut edits = Vec::new();
				for other in inside() {
					let level = other.level as isize + delta;
					if !(1..=6).contains(&level) {
						return Err(match delta {
							-1 => "a level 1 heading cannot be promoted",
							_ => "a level 6 heading cannot be demoted",
						}
						.into());
					}
					edits.push((other.marker.clone(), "#".repeat(level as usize)));
				}
				edits
			}
			MOVE_UP_COMMAND => {
				// The sibling before: the previous heading of its level, with nothing of a
				// higher level between them.
				let previous = (headings[..index].iter().enumerate().rev())
					.find(|(_, other)| other.level <= heading.level)
					.filter(|(_, other)| other.level == heading.level)
					.map(|(at, _)| at)
					.ok_or("no section of the same level before this one")?;
				let start = headings[previous].line_start;
				vec![(start..end, swap(text, start, heading.line_start, end))]
			}
			MOVE_DOWN_COMMAND => {
				let next = (headings[index + 1..].iter().enumerate())
					.find(|(_, other)| other.level <= heading.level)
					.filter(|(_, other)| other.level == heading.level)
					.map(|(at, _)| index + 1 + at)
					.ok_or("no section of the same level after this one")?;
				let next_end = section_end(&headings, next, text);
				let start = heading.line_start;
				let middle = headings[next].line_start;
				vec![(start..next_end, swap(text, start, middle, next_end))]
			}
			_ => return Err(format!("unknown command `{command}`").into()),
		})
	}

	/// Runs a structure command: sends its edit to the client.
	pub(crate) fn structure_command(
		&mut self,
		command: &str,
		arguments: &[serde_json::Value],
	) -> Result<()> {
		let (Some(uri), Some(line)) = (
			arguments.first().and_then(serde_json::Value::as_str),
			arguments.get(1).and_then(serde_json::Value::as_u64),
		) else {
			return Err(format!("`{command}` takes a document URI and a line").into());
		};
		let url = Url::parse(uri)?;
		let edits = self.structure_edits(command, &url, line)?;
		let source = &self.analyses[&url].source;
		let edits: Vec<_> = (edits.into_iter())
			.map(|(range, text)| {
				let token = Token::new(source.clone(), range);
				TextEdit::new(crate::convert::range(&token), text)
			})
			.collect();
		#[allow(clippy::mutable_key_type)] // See `actions.rs`.
		let changes = HashMap::from([(Uri::from_str(uri)?, edits)]);
		let params = lsp_types::ApplyWorkspaceEditParams {
			label: None,
			edit: WorkspaceEdit {
				changes: Some(changes),
				..WorkspaceEdit::default()
			},
		};
		self.send_request("workspace/applyEdit", serde_json::to_value(params)?);
		Ok(())
	}

	/// The structure commands that apply on `line`, a heading's.
	pub(crate) fn structure_actions(&self, url: &Url, line: u32) -> Vec<CodeActionOrCommand> {
		let Ok((analysis, headings, index)) = self.section_at(url, u64::from(line)) else {
			return Vec::new();
		};
		// On the heading's line only.
		let heading_line = analysis
			.source
			.position(headings[index].line_start, Encoding::Utf16)
			.map(|position| position.line as u32);
		if heading_line != Some(line) {
			return Vec::new();
		}
		let titles = [
			(PROMOTE_COMMAND, "Promote the section"),
			(DEMOTE_COMMAND, "Demote the section"),
			(MOVE_UP_COMMAND, "Move the section up"),
			(MOVE_DOWN_COMMAND, "Move the section down"),
		];
		(titles.into_iter())
			.filter(|(command, _)| self.structure_edits(command, url, u64::from(line)).is_ok())
			.map(|(command, title)| {
				CodeActionOrCommand::CodeAction(CodeAction {
					title: title.into(),
					kind: Some(CodeActionKind::new("refactor.rewrite")),
					command: Some(Command {
						title: title.into(),
						command: command.into(),
						arguments: Some(vec![url.as_str().into(), line.into()]),
					}),
					..CodeAction::default()
				})
			})
			.collect()
	}
}
