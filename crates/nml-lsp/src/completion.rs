//! Completion, from the text before the cursor: what is being typed rarely parses yet.

use std::collections::BTreeSet;

use lsp_types::CompletionItem;
use lsp_types::CompletionItemKind;
use lsp_types::CompletionParams;
use lsp_types::CompletionTextEdit;
use lsp_types::Documentation;
use lsp_types::InsertTextFormat;
use lsp_types::MarkupContent;
use lsp_types::MarkupKind;
use lsp_types::Position;
use lsp_types::Range;
use lsp_types::TextEdit;
use nml_document::Export;
use nml_grammar::Atom;
use nml_grammar::OptionKind;
use nml_grammar::OptionSchema;
use nml_grammar::RuleSet;
use nml_lang::Resolver;
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
	/// Markdown shown with the candidate.
	pub documentation: Option<String>,
	/// What is inserted instead of the label.
	pub text: Option<String>,
	/// A snippet inserted instead, for clients that take them.
	pub snippet: Option<String>,
	/// What the typed text is matched against instead of the label.
	pub filter: Option<String>,
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
		documentation: None,
		text: None,
		snippet: None,
		filter: None,
	}
}

impl Candidate {
	fn documented(mut self, documentation: impl Into<String>) -> Self {
		let documentation = documentation.into();
		self.documentation = (!documentation.is_empty()).then_some(documentation);
		self
	}

	fn inserting(mut self, text: impl Into<String>) -> Self {
		self.text = Some(text.into());
		self
	}

	fn with_snippet(mut self, snippet: &str) -> Self {
		self.snippet = (!snippet.is_empty()).then(|| snippet.to_string());
		self
	}

	fn filtered_by(mut self, filter: impl Into<String>) -> Self {
		self.filter = Some(filter.into());
		self
	}

	/// The candidate for text that starts with `typed`, which its label does not: it is
	/// matched after `typed` and inserted after `lead`.
	fn led_by(self, typed: &str, lead: &str) -> Self {
		let filter = format!("{typed}{}", self.label);
		match (&self.text, lead) {
			// A snippet alone stays one: it is not inserted as its label.
			(None, "") => self.filtered_by(filter),
			(text, _) => {
				let text = format!("{lead}{}", text.as_deref().unwrap_or(&self.label));
				self.inserting(text).filtered_by(filter)
			}
		}
	}
}

/// What a rule's documentation names, as plain text for a detail (editors show details as
/// code, not Markdown): its leading `**Name**`.
fn summary(doc: &str) -> Option<String> {
	let name = doc.strip_prefix("**")?.split("**").next()?.trim();
	(!name.is_empty()).then(|| name.to_string())
}

/// What the text before the cursor is in the middle of, and the length (in bytes) of the
/// part being typed, which the completion replaces.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Context<'a> {
	/// After `[[`: a unit key or a local symbol.
	Reference,
	/// After a lone `[`, which the text being typed starts with: a reference, its second `[`
	/// not typed yet.
	Bracket,
	/// After `[[doc#`, or `[[#` with an empty `doc`: a symbol.
	Symbol { unit: &'a str },
	/// A `:word` command (at the start of the line) or role.
	Command { line_start: bool },
	/// After `:import `.
	Module,
	/// After the fence of a code block: its language.
	CodeLanguage,
	/// After `> [!`: the kind of a callout.
	Callout,
	/// After an unclosed `%`: a variable.
	Variable,
	/// A word alone at the start of a line: the snippets of the grammar.
	Snippet,
	/// A `>` at the start of a line, which the text being typed starts with: quotes and
	/// callouts.
	Quote,
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
	// An odd number of unescaped `%`: a variable is being written.
	let percents = prefix.match_indices('%');
	let unescaped: Vec<_> = percents
		.filter(|(i, _)| !prefix[..*i].ends_with('\\'))
		.map(|(i, _)| i)
		.collect();
	if unescaped.len() % 2 == 1 {
		let name = &prefix[unescaped.last().unwrap() + 1..];
		if name
			.chars()
			.all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.'))
		{
			return Some((Context::Variable, name));
		}
	}
	if prefix.chars().all(|c| c.is_alphanumeric() || c == '_') {
		return Some((Context::Snippet, prefix));
	}
	if prefix
		.strip_prefix('>')
		.is_some_and(|rest| rest.chars().all(|c| c.is_alphanumeric()))
	{
		return Some((Context::Quote, prefix));
	}
	let mut quoted = prefix;
	while let Some(rest) = quoted.strip_prefix("> ") {
		quoted = rest;
	}
	if quoted.len() < prefix.len()
		&& let Some(kind) = quoted.strip_prefix("[!")
		&& !kind.contains(']')
	{
		return Some((Context::Callout, kind));
	}
	let fence = prefix.len() - prefix.trim_start_matches('`').len();
	if fence >= 3 && !prefix[fence..].contains(['[', ' ']) {
		return Some((Context::CodeLanguage, &prefix[fence..]));
	}
	if (prefix.starts_with(':') || fence >= 3)
		&& let Some(open) = prefix.find('[')
		&& !prefix[open..].contains(']')
	{
		// A code block's options belong to its fence, whatever the language.
		let opener = if fence >= 3 { "```" } else { &prefix[..open] };
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
	if let Some(open) = word_start
		.checked_sub(1)
		.filter(|i| prefix[*i..].starts_with('['))
	{
		let before = &prefix[..open];
		// Not media's `![`, its options' `)[`, an escaped `\[` or a checkbox's `* [`.
		let checkbox = !before.trim().is_empty()
			&& before.trim().chars().all(|c| matches!(c, '*' | '-'))
			&& before.ends_with(' ');
		if !before.ends_with(['[', '!', ')', '\\']) && !checkbox {
			return Some((Context::Bracket, &prefix[open..]));
		}
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
		let opener = rule.opener();
		if let Some(opener) = opener
			.as_ref()
			.and_then(|o| o.literal())
			.filter(|o| o.starts_with(':'))
		{
			openers.insert((opener.trim_end().to_string(), rule.name.clone()));
		}
	}
	openers
		.into_iter()
		.map(|(opener, name)| {
			let rule = rules.iter().find(|(_, rule)| rule.name == name).unwrap().1;
			candidate(opener, Some(name), CompletionItemKind::KEYWORD)
				.documented(&rule.doc)
				.with_snippet(&rule.snippet)
		})
		.collect()
}

/// The rules with a snippet, by name (`code block`), to write them from scratch.
pub(crate) fn snippets(rules: &RuleSet) -> Vec<Candidate> {
	let mut candidates: Vec<_> = (rules.iter())
		.filter(|(_, rule)| !rule.snippet.is_empty())
		.map(|(_, rule)| {
			candidate(
				rule.name.replace(['_', '-'], " "),
				summary(&rule.doc),
				CompletionItemKind::SNIPPET,
			)
			.documented(&rule.doc)
			.with_snippet(&rule.snippet)
		})
		.collect();
	candidates.sort_by(|a, b| a.label.cmp(&b.label));
	candidates
}

/// What a `>` at the start of a line begins: the rules whose snippet starts with it (a quote
/// and a callout), and a callout of each kind.
pub(crate) fn quotes(rules: &RuleSet) -> Vec<Candidate> {
	let mut candidates: Vec<_> = snippets(rules)
		.into_iter()
		.filter(|candidate| {
			candidate
				.snippet
				.as_ref()
				.is_some_and(|s| s.starts_with('>'))
		})
		.map(|candidate| match candidate.label.as_str() {
			"quote" => candidate.inserting("> "),
			_ => candidate,
		})
		.collect();
	let callout = rules.iter().find(|(_, rule)| rule.name == "callout");
	let doc = callout.map_or("", |(_, rule)| rule.doc.as_str());
	for kind in nml_document::CalloutKind::ALL.map(|kind| kind.name()) {
		candidates.push(
			candidate(kind, Some("Callout".into()), CompletionItemKind::SNIPPET)
				.documented(doc)
				.inserting(format!("> [!{kind}] "))
				.with_snippet(&format!("> [!{kind}] ${{1:Title}}\n> ${{0}}")),
		);
	}
	candidates.push(
		candidate(
			"cited quote",
			Some("Callout".into()),
			CompletionItemKind::SNIPPET,
		)
		.documented(doc)
		.inserting("> [!quote][author=")
		.with_snippet("> [!quote][author=${1:Author}]\n> ${0}"),
	);
	candidates
		.into_iter()
		.map(|candidate| candidate.led_by(">", ""))
		.collect()
}

/// The languages of code blocks: their names, as one word in lower case, and their file
/// extensions.
fn code_languages() -> Vec<Candidate> {
	let mut seen = BTreeSet::new();
	let mut candidates = Vec::new();
	for (name, extensions) in nml_render::languages() {
		let word = name.to_lowercase();
		let words = (!word.contains(' ')).then_some(word);
		for label in words.into_iter().chain(extensions.iter().cloned()) {
			if seen.insert(label.clone()) {
				candidates.push(candidate(
					label,
					Some(name.into()),
					CompletionItemKind::ENUM_MEMBER,
				));
			}
		}
	}
	candidates
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
		.find(|(_, rule)| {
			rule.opener()
				.is_some_and(|o| o.literal().is_some_and(|o| o.trim_end() == opener))
		})
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
		let suffix = &source.content()[offset..line.end];
		if let Some(items) = self.footnote_completion(analysis, prefix) {
			return Ok(items);
		}
		let Some((context, typed)) = context(prefix) else {
			return Ok(Vec::new());
		};
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let candidates = match context {
			Context::Reference => self.reference_candidates(analysis, suffix)?,
			Context::Bracket => (self.reference_candidates(analysis, suffix)?.into_iter())
				.map(|candidate| candidate.led_by("[", "[["))
				.collect(),
			Context::Quote => quotes(language.rules())
				.into_iter()
				.filter(|candidate| self.snippets || candidate.text.is_some())
				.collect(),
			Context::Symbol { unit } => {
				let close = |name: &str| {
					let close = !suffix.starts_with("]]") && !suffix.starts_with('|');
					format!("{name}{}", if close { "]]" } else { "" })
				};
				let mut candidates = Vec::new();
				// This unit's own first: its buffer may not be saved.
				if unit.is_empty() || unit == analysis.key {
					for export in &analysis.exports {
						let title = export.property("title").map(String::from);
						candidates.push(
							candidate(&export.name, title, CompletionItemKind::REFERENCE)
								.documented(self.export_doc(analysis, export))
								.inserting(close(&export.name)),
						);
					}
				}
				if let Some(index) = self.index() {
					for (key, name, title) in index.all_exports()? {
						let own = key == analysis.key
							&& candidates.iter().any(|c: &Candidate| c.label == name);
						if (unit.is_empty() || key == unit) && !own {
							let detail = title.as_ref().map(|title| format!("{title} ({key})"));
							let doc = format!(
								"**{}**\n\n`{name}` in `{key}`",
								title.as_deref().unwrap_or(&name)
							);
							candidates.push(
								candidate(&name, detail, CompletionItemKind::REFERENCE)
									.documented(doc)
									.inserting(close(&name)),
							);
						}
					}
				}
				candidates
			}
			Context::Variable => {
				let close = if suffix.starts_with('%') { "" } else { "%" };
				let mut names: Vec<_> = analysis.variables.keys().collect();
				names.sort();
				names
					.into_iter()
					.map(|name| {
						let variable = &analysis.variables[name];
						let detail = Some(variable.value.content().trim().to_string());
						candidate(name, detail, CompletionItemKind::VARIABLE)
							.documented(self.variable_doc(name, variable))
							.inserting(format!("{name}{close}"))
					})
					.collect()
			}
			Context::Snippet if self.snippets => snippets(language.rules()),
			Context::Snippet => Vec::new(),
			Context::Command { line_start } => commands(language.rules(), line_start),
			Context::Module => {
				let resolver = self.resolver();
				self.module_paths()
					.into_iter()
					.map(|module| {
						let doc = (resolver.module(&module).ok())
							.map(|(_, source)| nml_lang::module_doc(source.content()))
							.unwrap_or_default();
						candidate(module, None, CompletionItemKind::MODULE).documented(doc)
					})
					.collect()
			}
			Context::CodeLanguage => code_languages(),
			Context::Callout => std::iter::once("quote")
				.chain(nml_document::CalloutKind::ALL.map(|kind| kind.name()))
				.map(|kind| candidate(kind, None, CompletionItemKind::ENUM_MEMBER))
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
			.filter(|candidate| {
				(candidate.filter.as_ref())
					.unwrap_or(&candidate.label)
					.starts_with(typed)
			})
			.map(|candidate| {
				let (text, format) = match candidate.snippet {
					Some(snippet) if self.snippets => (snippet, InsertTextFormat::SNIPPET),
					_ => (
						candidate.text.unwrap_or_else(|| candidate.label.clone()),
						InsertTextFormat::PLAIN_TEXT,
					),
				};
				CompletionItem {
					label: candidate.label,
					filter_text: candidate.filter,
					kind: Some(candidate.kind),
					detail: candidate.detail,
					documentation: candidate.documentation.map(|value| {
						Documentation::MarkupContent(MarkupContent {
							kind: MarkupKind::Markdown,
							value,
						})
					}),
					insert_text_format: Some(format),
					text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(range, text))),
					..CompletionItem::default()
				}
			})
			.collect())
	}

	/// The references that can be written after `[[`: this unit's symbols, then the pages
	/// of the workspace.
	fn reference_candidates(
		&self,
		analysis: &crate::Analysis,
		suffix: &str,
	) -> Result<Vec<Candidate>> {
		let close = !suffix.starts_with("]]") && !suffix.starts_with('|');
		let mut candidates: Vec<_> = analysis
			.exports
			.iter()
			.map(|export| {
				let title = export.property("title").map(String::from);
				let text = format!("{}{}", export.name, if close { "]]" } else { "" });
				candidate(&export.name, title, CompletionItemKind::REFERENCE)
					.documented(self.export_doc(analysis, export))
					.inserting(text)
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
				let doc = format!("**{}**\n\nThe page `{}`.", unit.title, unit.path);
				candidates.push(
					candidate(format!("{}#", unit.name), detail, CompletionItemKind::FILE)
						.documented(doc),
				);
			}
		}
		Ok(candidates)
	}

	/// The documentation of a symbol of the document: its label, title and kind.
	pub(crate) fn export_doc(&self, analysis: &crate::Analysis, export: &Export) -> String {
		let target = nml_build::Target {
			unit: self.current_unit(analysis),
			export: Some(export.clone()),
		};
		let label = nml_build::label(&target);
		let title = export.property("title").unwrap_or(&export.name);
		let mut doc = format!("**{label}**");
		if title != label {
			doc.push_str(&format!(": {title}"));
		}
		let kind = format!("{:?}", export.kind).to_lowercase();
		let path = match &export.declared {
			Some((path, _)) => path.as_str(),
			None => &analysis.path,
		};
		doc.push_str(&format!("\n\n{kind} `{}` in `{path}`", export.name));
		doc
	}

	/// The documentation of a variable: its value, and where it is set.
	pub(crate) fn variable_doc(&self, name: &str, variable: &nml_lang::Variable) -> String {
		let value = variable.value.content().trim();
		let root = variable.name.root();
		let source = root.source();
		let file = self
			.workspace_path(source.location())
			.unwrap_or_else(|| source.location().path().to_string());
		let line = source
			.position(root.start(), Encoding::Utf8)
			.map_or(0, |position| position.line + 1);
		format!("**`%{name}%`**\n\n```nml\n{value}\n```\n\nSet in `{file}`, line {line}.")
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
		// The modules that come with NML, unless the workspace has its own `std`.
		if !config.modules.iter().any(|module| module.name == "std") {
			paths.extend(nml_lua::STD.iter().map(|(name, _)| format!("std.{name}")));
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
		// Variables: after an unclosed `%`, not an escaped one.
		assert_eq!(context("Hi %na"), Some((Context::Variable, "na")));
		assert_eq!(
			context("%a% and %lib.si"),
			Some((Context::Variable, "lib.si"))
		);
		assert_eq!(context("%a% and"), None);
		assert_eq!(context("50\\% and %"), Some((Context::Variable, "")));
		assert_eq!(context("50\\% off"), None);
		assert_eq!(context("%a b"), None);
		// A word alone at the start of a line, or nothing: snippets.
		assert_eq!(context("fig"), Some((Context::Snippet, "fig")));
		assert_eq!(context(""), Some((Context::Snippet, "")));
		assert_eq!(context(" fig"), None);
		// A lone `[`: a reference, but not media, options, an escape or a checkbox.
		assert_eq!(context("See ["), Some((Context::Bracket, "[")));
		assert_eq!(context("["), Some((Context::Bracket, "[")));
		assert_eq!(context("See [gu"), Some((Context::Bracket, "[gu")));
		assert_eq!(context("[[a]] and [x"), Some((Context::Bracket, "[x")));
		assert_eq!(context("See [two words"), None);
		assert_eq!(context("!["), None);
		assert_eq!(context("![a](b.png)["), None);
		assert_eq!(context("\\["), None);
		assert_eq!(context("* ["), None);
		assert_eq!(context("** ["), None);
		assert_eq!(context("- [ "), None);
		assert_eq!(context("a * ["), Some((Context::Bracket, "[")));
		// A `>` at the start of a line: quotes and callouts.
		assert_eq!(context(">"), Some((Context::Quote, ">")));
		assert_eq!(context(">to"), Some((Context::Quote, ">to")));
		assert_eq!(context("> to"), None);
		assert_eq!(context("a >"), None);
		assert_eq!(context("```ru"), Some((Context::CodeLanguage, "ru")));
		assert_eq!(context("> [!wa"), Some((Context::Callout, "wa")));
		assert_eq!(context("> > [!"), Some((Context::Callout, "")));
		assert_eq!(context("> [!note] x"), None);
		assert_eq!(context("[!note"), None);
		assert_eq!(context("````"), Some((Context::CodeLanguage, "")));
		assert_eq!(context("```rust Title"), None);
		assert_eq!(
			context("```rust[line"),
			Some((
				Context::Option {
					opener: "```",
					key: None,
					given: vec![]
				},
				"line"
			))
		);
		assert_eq!(
			code_languages()
				.into_iter()
				.find(|c| c.label == "rs")
				.and_then(|c| c.detail)
				.as_deref(),
			Some("Rust")
		);
		assert!(code_languages().iter().any(|c| c.label == "rust"));
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
