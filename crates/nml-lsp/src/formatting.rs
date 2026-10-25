//! `textDocument/formatting` and `rangeFormatting`: tables are realigned, as org-mode does
//! on TAB. Cells are padded to their column's width, on the side the separator row aligns
//! them to, and the pipes line up. Nothing outside tables changes, and tables with cell
//! options (`[hspan=2]` right after a pipe) are left as written.

use std::ops::Range;

use lsp_types::CodeAction;
use lsp_types::CodeActionKind;
use lsp_types::CodeActionOrCommand;
use lsp_types::TextEdit;
use lsp_types::WorkspaceEdit;
use nml_grammar::Node;
use nml_grammar::Value;
use nml_source::Token;
use nml_source::Url;
use unicode_width::UnicodeWidthStr;

use crate::Result;
use crate::Server;
use crate::convert;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
	Default,
	Left,
	Right,
	Center,
}

/// The cells of a row, `| a | b |`, trimmed: split on the pipes that are not escaped or in
/// inline code. `None` if it is not a row this can align: no closing pipe, or cell options.
fn cells(line: &str) -> Option<Vec<String>> {
	let line = line.trim();
	let inner = line.strip_prefix('|')?.strip_suffix('|')?;
	let mut cells = Vec::new();
	let mut cell = String::new();
	let mut chars = inner.chars().peekable();
	// The length of the backtick run that opened inline code, if in it.
	let mut code: Option<usize> = None;
	while let Some(c) = chars.next() {
		match c {
			'\\' => {
				cell.push(c);
				if let Some(next) = chars.next() {
					cell.push(next);
				}
			}
			'`' => {
				let mut run = 1;
				while chars.peek() == Some(&'`') {
					chars.next();
					run += 1;
				}
				cell.push_str(&"`".repeat(run));
				code = match code {
					None => Some(run),
					Some(open) if open == run => None,
					open => open,
				};
			}
			'|' if code.is_none() => cells.push(std::mem::take(&mut cell)),
			c => cell.push(c),
		}
	}
	cells.push(cell);
	// Options right after a pipe are part of their cell's syntax.
	if cells.iter().any(|cell| cell.starts_with('[')) {
		return None;
	}
	Some(
		cells
			.into_iter()
			.map(|cell| cell.trim().to_string())
			.collect(),
	)
}

/// The alignments of a separator row, `|:---|--:|`, or `None` if it is not one.
fn separator(cells: &[String]) -> Option<Vec<Align>> {
	cells
		.iter()
		.map(|cell| {
			let dashes = cell.trim_start_matches(':').trim_end_matches(':');
			if dashes.is_empty() || !dashes.chars().all(|c| c == '-') {
				return None;
			}
			Some(match (cell.starts_with(':'), cell.ends_with(':')) {
				(true, true) => Align::Center,
				(true, false) => Align::Left,
				(false, true) => Align::Right,
				(false, false) => Align::Default,
			})
		})
		.collect()
}

/// The lines of a table, aligned, or `None` if it cannot be aligned. `lines` are its rows,
/// without their line breaks.
fn align(lines: &[&str]) -> Option<Vec<String>> {
	let rows: Vec<Vec<String>> = lines
		.iter()
		.map(|line| cells(line))
		.collect::<Option<_>>()?;
	let separators: Vec<Option<Vec<Align>>> = rows.iter().map(|row| separator(row)).collect();
	let columns = rows.iter().map(Vec::len).max()?;
	let aligns = separators
		.iter()
		.flatten()
		.next()
		.cloned()
		.unwrap_or_default();
	let mut widths = vec![0; columns];
	for (row, separator) in rows.iter().zip(&separators) {
		if separator.is_some() {
			continue;
		}
		for (column, cell) in row.iter().enumerate() {
			widths[column] = widths[column].max(cell.width());
		}
	}
	// A separator needs three characters.
	for width in &mut widths {
		*width = (*width).max(3);
	}
	let indent = &lines[0][..lines[0].len() - lines[0].trim_start().len()];
	let mut out = Vec::new();
	for (row, separator) in rows.iter().zip(&separators) {
		let mut line = indent.to_string();
		line.push('|');
		for (column, cell) in row.iter().enumerate() {
			let width = widths[column];
			let text = match separator {
				Some(aligns) => {
					let dashes = width;
					match aligns[column] {
						Align::Default => "-".repeat(dashes + 2),
						Align::Left => format!(":{}", "-".repeat(dashes + 1)),
						Align::Right => format!("{}:", "-".repeat(dashes + 1)),
						Align::Center => format!(":{}:", "-".repeat(dashes)),
					}
				}
				None => {
					let pad = width - cell.width();
					let (left, right) = match aligns.get(column).copied().unwrap_or(Align::Default)
					{
						Align::Right => (pad, 0),
						Align::Center => (pad / 2, pad - pad / 2),
						_ => (0, pad),
					};
					format!(" {}{cell}{} ", " ".repeat(left), " ".repeat(right))
				}
			};
			line.push_str(&text);
			line.push('|');
		}
		out.push(line);
	}
	Some(out)
}

/// The tables of a tree written in the file: the byte ranges of their rows. Rows in a quote
/// have its markers in between, and are not aligned.
fn tables(nodes: &[Node], language: &nml_lang::Language, file: &Url, out: &mut Vec<Range<usize>>) {
	for node in nodes {
		match node {
			Node::Paragraph { children, .. } => tables(children, language, file, out),
			Node::Match(m) => {
				let name = language.rules().get(m.rule).map(|rule| rule.name.as_str());
				// A `:table` holds one in its block.
				if name == Some("table_rows") && m.token.root().source().location() == file {
					let root = m.token.root();
					let content = root.source().content();
					let end = content[..root.end()].trim_end().len();
					if root.start() < end {
						out.push(root.start()..end);
					}
					continue;
				}
				for capture in &m.captures {
					if let Value::Nodes(nodes) = &capture.value {
						tables(nodes, language, file, out);
					}
				}
			}
			_ => {}
		}
	}
}

impl Server {
	/// The edits that align the tables of a document, those in `range` only if given.
	pub(crate) fn table_edits(
		&self,
		url: &Url,
		range: Option<lsp_types::Range>,
	) -> Result<Vec<TextEdit>> {
		let Some(analysis) = self.analyses.get(url) else {
			return Ok(Vec::new());
		};
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let mut found = Vec::new();
		tables(&analysis.tree, language, url, &mut found);
		let text = analysis.source.content();
		let mut edits = Vec::new();
		for table in found {
			let token = Token::new(analysis.source.clone(), table.clone());
			let lines_range = convert::range(&token);
			if let Some(range) = range
				&& (lines_range.end.line < range.start.line
					|| lines_range.start.line > range.end.line)
			{
				continue;
			}
			let lines: Vec<&str> = text[table.clone()].lines().collect();
			let Some(aligned) = align(&lines) else {
				continue;
			};
			let aligned = aligned.join("\n");
			if aligned != text[table.clone()] {
				edits.push(TextEdit::new(lines_range, aligned));
			}
		}
		Ok(edits)
	}

	/// "Align the table" when the cursor is in a table that is not aligned.
	pub(crate) fn table_actions(
		&self,
		url: &Url,
		range: lsp_types::Range,
	) -> Vec<CodeActionOrCommand> {
		let edits = self.table_edits(url, Some(range)).unwrap_or_default();
		if edits.is_empty() {
			return Vec::new();
		}
		let Ok(uri) = std::str::FromStr::from_str(url.as_str()) else {
			return Vec::new();
		};
		#[allow(clippy::mutable_key_type)] // See `actions.rs`.
		let changes = std::collections::HashMap::from([(uri, edits)]);
		vec![CodeActionOrCommand::CodeAction(CodeAction {
			title: "Align the table".into(),
			kind: Some(CodeActionKind::new("refactor.rewrite")),
			edit: Some(WorkspaceEdit {
				changes: Some(changes),
				..WorkspaceEdit::default()
			}),
			..CodeAction::default()
		})]
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn aligned(text: &str) -> Option<String> {
		let lines: Vec<&str> = text.lines().collect();
		align(&lines).map(|lines| lines.join("\n"))
	}

	#[test]
	fn alignment() {
		assert_eq!(
			aligned(
				"| Fruit | Price | Stock |\n|:-|-:|:-:|\n| Apples | 2.50 | yes |\n| Pears | 3.1 | no |"
			)
			.unwrap(),
			"| Fruit  | Price | Stock |\n\
			 |:-------|------:|:-----:|\n\
			 | Apples |  2.50 |  yes  |\n\
			 | Pears  |   3.1 |  no   |"
		);
		// Without a separator; short rows keep their cells; wide characters.
		assert_eq!(
			aligned("|a|b|\n|longer|\n|日本|x|").unwrap(),
			"| a      | b   |\n| longer |\n| 日本   | x   |"
		);
		// Pipes in code and escaped ones are in their cell.
		assert_eq!(
			aligned("| `a|b` | c\\|d |\n| e | f |").unwrap(),
			"| `a|b` | c\\|d |\n| e     | f    |"
		);
		// Indented tables stay indented.
		assert_eq!(
			aligned("  |a|bb|\n  |ccc|d|").unwrap(),
			"  | a   | bb  |\n  | ccc | d   |"
		);
	}

	#[test]
	fn left_alone() {
		assert_eq!(aligned("| a |[hspan=2] b |\n| c | d | e |"), None);
		assert_eq!(aligned("| a | b\n| c | d |"), None);
	}
}
