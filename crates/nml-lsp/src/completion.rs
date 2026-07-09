//! Completion, from the text before the cursor: what is being typed rarely parses yet.

use std::collections::BTreeSet;

use lsp_types::CompletionItem;
use lsp_types::CompletionItemKind;
use lsp_types::CompletionParams;
use lsp_types::CompletionTextEdit;
use lsp_types::Position;
use lsp_types::Range;
use lsp_types::TextEdit;
use nml_grammar::Atom;
use nml_grammar::OptionKind;
use nml_grammar::OptionSchema;
use nml_grammar::RuleSet;
use nml_source::Encoding;
use nml_source::Url;

use crate::Result;
use crate::Server;

/// A completion: what replaces the text being typed, and a detail shown beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
	pub label: String,
	pub detail: Option<String>,
	pub kind: CompletionItemKind,
}

fn candidate(
	label: impl Into<String>,
	detail: Option<String>,
	kind: CompletionItemKind,
) -> Candidate {
	Candidate {
		label: label.into(),
		detail,
		kind,
	}
}

/// What the text before the cursor is in the middle of, and the length (in bytes) of the
/// part being typed, which the completion replaces.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Context<'a> {
	/// After `[[`: a unit key or a local symbol.
	Reference,
	/// After `[[doc#`, or `[[#` with an empty `doc`: a symbol.
	Symbol { unit: &'a str },
	/// A `:word` command (at the start of the line) or role.
	Command { line_start: bool },
	/// After `:import `.
	Module,
	/// Inside the option list of the command `opener`: a name, or the value of `key`.
	Option {
		opener: &'a str,
		key: Option<&'a str>,
		given: Vec<&'a str>,
	},
}

/// The context of the text before the cursor on its line, and the part being typed.
pub(crate) fn context(prefix: &str) -> Option<(Context<'_>, &str)> {
	let word_start = prefix
		.rfind(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-' || c == '.'))
		.map_or(0, |i| i + prefix[i..].chars().next().unwrap().len_utf8());
	let typed = &prefix[word_start..];

	if let Some(rest) = prefix.strip_prefix(":import ") {
		return Some((Context::Module, rest));
	}
	if let Some(start) = prefix.rfind("[[") {
		let inside = &prefix[start + 2..];
		if !inside.contains("]]") && !inside.contains('|') {
			return Some(match inside.split_once('#') {
				Some((unit, symbol)) => (Context::Symbol { unit }, symbol),
				None => (Context::Reference, inside),
			});
		}
	}
	if prefix.starts_with(':')
		&& let Some(open) = prefix.find('[')
		&& !prefix[open..].contains(']')
	{
		let opener = &prefix[..open];
		let entries: Vec<_> = prefix[open + 1..].split(',').collect();
		let (current, done) = entries.split_last().unwrap();
		let given = done
			.iter()
			.map(|entry| entry.split('=').next().unwrap().trim())
			.collect();
		return Some(match current.split_once('=') {
			Some((key, value)) => (
				Context::Option {
					opener,
					key: Some(key.trim()),
					given,
				},
				value.trim_start(),
			),
			None => (
				Context::Option {
					opener,
					key: None,
					given,
				},
				current.trim_start(),
			),
		});
	}
	let colon = word_start
		.checked_sub(1)
		.filter(|i| prefix[*i..].starts_with(':'))?;
	let line_start = colon == 0;
	(!typed.contains('.')).then_some((Context::Command { line_start }, &prefix[colon..]))
}

/// The commands and roles of a grammar: the openers that start with `:`.
pub(crate) fn commands(rules: &RuleSet, line_start: bool) -> Vec<Candidate> {
	let mut openers = BTreeSet::new();
	for (_, rule) in rules.iter() {
		let block = rule.table() == nml_grammar::Table::Block;
		if block && !line_start {
			continue;
		}
		if let Some(opener) = rule.opener().filter(|opener| opener.starts_with(':')) {
			openers.insert((opener.trim_end().to_string(), rule.name.clone()));
		}
	}
	openers
		.into_iter()
		.map(|(opener, name)| candidate(opener, Some(name), CompletionItemKind::KEYWORD))
		.collect()
}

/// The option schema of the command whose opener is `opener`.
pub(crate) fn schema<'r>(rules: &'r RuleSet, opener: &str) -> Option<&'r OptionSchema> {
	fn find(atoms: &[Atom]) -> Option<&OptionSchema> {
		atoms.iter().find_map(|atom| match atom {
			Atom::Options(schema) => Some(&**schema),
			Atom::Capture { atom, .. } => find(std::slice::from_ref(atom)),
			Atom::Repeat { atoms, .. } => find(atoms),
			Atom::Or(alternatives) => alternatives.iter().find_map(|atoms| find(atoms)),
			_ => None,
		})
	}
	rules
		.iter()
		.find(|(_, rule)| rule.opener().is_some_and(|o| o.trim_end() == opener))
		.and_then(|(_, rule)| find(&rule.atoms))
}

/// Option names not given yet, or the values of `key`.
pub(crate) fn options(schema: &OptionSchema, key: Option<&str>, given: &[&str]) -> Vec<Candidate> {
	match key {
		None => schema
			.options
			.iter()
			.filter(|option| !given.contains(&option.name.as_str()))
			.map(|option| {
				let detail = format!(
					"{}{}",
					option.kind,
					if option.doc.is_empty() {
						String::new()
					} else {
						format!(": {}", option.doc)
					}
				);
				candidate(&option.name, Some(detail), CompletionItemKind::PROPERTY)
			})
			.collect(),
		Some(key) => match schema.get(key).map(|option| &option.kind) {
			Some(OptionKind::Choice(choices)) => choices
				.iter()
				.map(|choice| candidate(choice, None, CompletionItemKind::ENUM_MEMBER))
				.collect(),
			Some(OptionKind::Bool) => ["true", "false"]
				.into_iter()
				.map(|value| candidate(value, None, CompletionItemKind::VALUE))
				.collect(),
			_ => Vec::new(),
		},
	}
}

impl Server {
	pub(crate) fn completion(&self, params: &CompletionParams) -> Result<Vec<CompletionItem>> {
		let position = &params.text_document_position;
		let url = Url::parse(position.text_document.uri.as_str())?;
		let Some(analysis) = self.analyses.get(&url) else {
			return Ok(Vec::new());
		};
		let source = &analysis.source;
		let cursor = nml_source::Position::new(
			position.position.line as usize,
			position.position.character as usize,
		);
		let Some(offset) = source.offset(cursor, Encoding::Utf16) else {
			return Ok(Vec::new());
		};
		let line = source.line_range(cursor.line).unwrap();
		let prefix = &source.content()[line.start..offset];
		let Some((context, typed)) = context(prefix) else {
			return Ok(Vec::new());
		};
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let candidates = match context {
			Context::Reference => {
				let mut candidates: Vec<_> = analysis
					.exports
					.iter()
					.map(|export| {
						let title = export.property("title").map(String::from);
						candidate(&export.name, title, CompletionItemKind::REFERENCE)
					})
					.collect();
				if let Some(index) = self.index() {
					// Only pages: a link to a library has no page to point to.
					for unit in index
						.units()?
						.into_iter()
						.filter(|u| u.role == nml_workspace::Role::Page)
					{
						let detail = Some(format!("{} ({})", unit.title, unit.path));
						candidates.push(candidate(
							format!("{}#", unit.name),
							detail,
							CompletionItemKind::FILE,
						));
					}
				}
				candidates
			}
			Context::Symbol { unit } => {
				let mut candidates = Vec::new();
				if let Some(index) = self.index() {
					for (key, name, title) in index.all_exports()? {
						if unit.is_empty() || key == unit {
							let detail = title.map(|title| format!("{title} ({key})"));
							candidates.push(candidate(name, detail, CompletionItemKind::REFERENCE));
						}
					}
				}
				if unit.is_empty() || unit == analysis.key {
					for export in &analysis.exports {
						if !candidates.iter().any(|c| c.label == export.name) {
							let title = export.property("title").map(String::from);
							candidates.push(candidate(
								&export.name,
								title,
								CompletionItemKind::REFERENCE,
							));
						}
					}
				}
				candidates
			}
			Context::Command { line_start } => commands(language.rules(), line_start),
			Context::Module => self
				.module_paths()
				.into_iter()
				.map(|path| candidate(path, None, CompletionItemKind::MODULE))
				.collect(),
			Context::Option { opener, key, given } => match schema(language.rules(), opener) {
				Some(schema) => options(schema, key, &given),
				None => Vec::new(),
			},
		};

		let start = source
			.position(offset - typed.len(), Encoding::Utf16)
			.unwrap();
		let range = Range::new(
			Position::new(start.line as u32, start.column as u32),
			position.position,
		);
		Ok(candidates
			.into_iter()
			.filter(|candidate| candidate.label.starts_with(typed))
			.map(|candidate| CompletionItem {
				label: candidate.label.clone(),
				kind: Some(candidate.kind),
				detail: candidate.detail,
				text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
					range,
					candidate.label,
				))),
				..CompletionItem::default()
			})
			.collect())
	}

	/// The module paths of the `.nml` files under the module roots.
	fn module_paths(&self) -> Vec<String> {
		let (Some(root), Some(config)) = (&self.root, &self.config) else {
			return Vec::new();
		};
		let mut paths = Vec::new();
		for module in &config.modules {
			let mut stack = vec![root.join(&module.root)];
			while let Some(dir) = stack.pop() {
				let Ok(entries) = std::fs::read_dir(&dir) else {
					continue;
				};
				for entry in entries.flatten() {
					let path = entry.path();
					if path.is_dir() {
						stack.push(path);
					} else if path.extension().is_some_and(|e| e == "nml")
						&& let Ok(relative) = path.strip_prefix(root)
					{
						let relative = relative.to_string_lossy().replace('\\', "/");
						if let Some(module_path) = config.module_path(&relative) {
							paths.push(module_path);
						}
					}
				}
			}
		}
		paths.sort();
		paths
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use nml_grammar::OptionSpec;
	use nml_grammar::OptionValue;
	use nml_grammar::Rule;

	use super::*;

	#[test]
	fn contexts() {
		assert_eq!(context("See [[gu"), Some((Context::Reference, "gu")));
		assert_eq!(
			context("[[guide#in"),
			Some((Context::Symbol { unit: "guide" }, "in"))
		);
		assert_eq!(context("[[#in"), Some((Context::Symbol { unit: "" }, "in")));
		assert_eq!(context("[[a]] and [[b|cap"), None);
		assert_eq!(context("[[a]] text"), None);
		assert_eq!(
			context(":im"),
			Some((Context::Command { line_start: true }, ":im"))
		);
		assert_eq!(
			context("Press :k"),
			Some((Context::Command { line_start: false }, ":k"))
		);
		assert_eq!(context(":import std.u"), Some((Context::Module, "std.u")));
		assert_eq!(
			context(":graph[layout=ne"),
			Some((
				Context::Option {
					opener: ":graph",
					key: Some("layout"),
					given: vec![]
				},
				"ne"
			))
		);
		assert_eq!(
			context(":graph[layout=dot, sc"),
			Some((
				Context::Option {
					opener: ":graph",
					key: None,
					given: vec!["layout"]
				},
				"sc"
			))
		);
		assert_eq!(context("plain text"), None);
	}

	#[test]
	fn option_candidates() {
		let schema = OptionSchema::new([
			OptionSpec::new(
				"layout",
				OptionKind::Choice(vec!["dot".into(), "neato".into()]),
			)
			.with_default(OptionValue::Text("dot".into()))
			.with_doc("The engine's layout."),
			OptionSpec::new("inline", OptionKind::Bool),
			OptionSpec::new("scale", OptionKind::Number),
		]);
		let mut rules = RuleSet::new();
		rules
			.add(Rule::new(
				"graph",
				vec![
					Atom::LineStart,
					Atom::literal(":graph"),
					Atom::optional(vec![Atom::capture(
						"options",
						Atom::Options(Arc::new(schema)),
					)]),
					Atom::LineEnd,
				],
			))
			.unwrap();
		let schema = super::schema(&rules, ":graph").unwrap();
		let labels = |candidates: Vec<Candidate>| -> Vec<String> {
			candidates.into_iter().map(|c| c.label).collect()
		};
		assert_eq!(
			labels(options(schema, None, &["layout"])),
			["inline", "scale"]
		);
		assert_eq!(
			labels(options(schema, Some("layout"), &[])),
			["dot", "neato"]
		);
		assert_eq!(
			labels(options(schema, Some("inline"), &[])),
			["true", "false"]
		);
		assert!(options(schema, Some("scale"), &[]).is_empty());
		assert_eq!(
			options(schema, None, &[])[0].detail.as_deref(),
			Some("dot, neato: The engine's layout.")
		);
		assert!(super::schema(&rules, ":other").is_none());
	}
}
