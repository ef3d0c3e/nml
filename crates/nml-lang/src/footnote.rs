//! Footnotes: `[^name]` in a line refers to the note defined by `[^name]: text` at the start
//! of a line. Notes are numbered in the order they are first referred to, and shown in a
//! section at the end of the page.

use std::collections::HashMap;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::ElementKind;
use nml_grammar::Atom;
use nml_grammar::Prefix;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::RuleMatch;
use nml_source::Token;

use crate::Language;
use crate::Lower;

/// The footnotes of a unit.
#[derive(Default)]
struct Footnotes {
	/// The notes referred to, by name: their number and how many times.
	numbers: HashMap<String, (u32, u32)>,
	/// The references, in order, with their name.
	references: Vec<(String, Token)>,
	/// The definitions, in order: their name, match, and the file they are written in.
	definitions: Vec<(Token, RuleMatch, String)>,
	/// Whether the section is to be written when the lowering ends.
	finishing: bool,
}

pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let reference = Rule::new(
		"footnote_ref",
		vec![
			Atom::capture("open", Atom::literal("[^")).with_class("operator"),
			Atom::capture("name", Atom::Word).with_class("variable"),
			Atom::capture("close", Atom::literal("]")).with_class("operator"),
		],
	)
	.with_doc(
		"**Footnote**: `[^name]` refers to the note `[^name]: text`, written anywhere in the \
		document. Notes are numbered in the order they are referred to, and shown at the end \
		of the page.",
	)
	.with_snippet("[^${1:name}]");
	language.add(reference, lower_reference)?;

	let definition = Rule::new(
		"footnote",
		vec![
			Atom::LineStart,
			Atom::capture("open", Atom::literal("[^")).with_class("operator"),
			Atom::capture("name", Atom::Word).with_class("variable"),
			Atom::capture("close", Atom::literal("]:")).with_class("operator"),
			Atom::Blanks,
			Atom::capture("body", Atom::Container(Prefix::Blanks)),
		],
	)
	.with_doc(
		"**Footnote's note**: `[^name]: text` at the start of a line, shown at the end of the \
		page for the references `[^name]`. Lines indented under its text continue it.",
	)
	.with_snippet("[^${1:name}]: ${0}");
	language.add(definition, lower_definition)?;
	Ok(())
}

fn lower_reference(lower: &mut Lower<'_, '_>, m: &RuleMatch) {
	let name = m.capture("name").unwrap().token.content().to_string();
	finish(lower);
	let footnotes = lower.extension(Footnotes::default);
	let next = footnotes.numbers.len() as u32 + 1;
	let (number, count) = footnotes.numbers.entry(name.clone()).or_insert((next, 0));
	*count += 1;
	let (number, occurrence) = (*number, *count);
	footnotes.references.push((name.clone(), m.token.clone()));
	let kind = ElementKind::FootnoteRef {
		name,
		number,
		occurrence,
	};
	lower.builder.leaf(kind, m.token.clone());
}

fn lower_definition(lower: &mut Lower<'_, '_>, m: &RuleMatch) {
	let name = m.capture("name").unwrap().token.clone();
	finish(lower);
	let file = lower.files.last().unwrap().clone();
	let footnotes = lower.extension(Footnotes::default);
	footnotes.definitions.push((name, m.clone(), file));
}

/// Writes the section of the notes when the lowering ends, once.
fn finish(lower: &mut Lower<'_, '_>) {
	let footnotes = lower.extension(Footnotes::default);
	if footnotes.finishing {
		return;
	}
	footnotes.finishing = true;
	lower.on_finish(|lower| {
		let Some(footnotes) = lower.extension_mut::<Footnotes>() else {
			return;
		};
		let numbers = std::mem::take(&mut footnotes.numbers);
		let references = std::mem::take(&mut footnotes.references);
		let definitions = std::mem::take(&mut footnotes.definitions);

		// Each name defined once: the first definition.
		let mut defined: Vec<(u32, &Token, &RuleMatch, &String)> = Vec::new();
		let mut seen: HashMap<&str, &Token> = HashMap::new();
		for (name, m, file) in &definitions {
			if let Some(first) = seen.get(name.content()) {
				let kind = DiagnosticKind::DuplicateFootnote {
					name: name.content().into(),
				};
				let diagnostic = Diagnostic::new(kind, DiagnosticMessage::new(name.clone()))
					.with(DiagnosticMessage::fluent((*first).clone(), "first"));
				lower.diagnostics.push(diagnostic);
				continue;
			}
			seen.insert(name.content(), name);
			match numbers.get(name.content()) {
				Some((number, _)) => defined.push((*number, name, m, file)),
				None => {
					let kind = DiagnosticKind::UnusedFootnote {
						name: name.content().into(),
					};
					let diagnostic = Diagnostic::new(kind, DiagnosticMessage::new(name.clone()));
					lower.diagnostics.push(diagnostic);
				}
			}
		}
		for (name, token) in &references {
			if !seen.contains_key(name.as_str()) {
				let kind = DiagnosticKind::UndefinedFootnote { name: name.clone() };
				let diagnostic = Diagnostic::new(kind, DiagnosticMessage::new(token.clone()));
				lower.diagnostics.push(diagnostic);
			}
		}
		if defined.is_empty() {
			return;
		}
		defined.sort_by_key(|(number, ..)| *number);
		let section = defined[0].2.token.clone();
		lower.builder.open(ElementKind::Footnotes, section);
		for (number, name, m, file) in defined {
			let references = numbers[name.content()].1;
			let kind = ElementKind::Footnote {
				name: name.content().into(),
				number,
				references,
			};
			lower.builder.open(kind, m.token.clone());
			// Paths in the note are relative to its file.
			lower.files.push(file.clone());
			lower.capture(m, "body");
			lower.files.pop();
			lower.builder.close();
		}
		lower.builder.close();
	});
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;

	use crate::tests::check;
	use crate::tests::compile_text;

	#[test]
	fn notes_at_the_end() {
		check(
			"Text[^b] and more[^a], again[^b].\n\n[^a]: The *first* defined.\n[^b]: Long\n      note.\n\n# After\n",
			r#"document[paragraph["Text" footnote_ref[] " and more" footnote_ref[] ", again" footnote_ref[] "."] h1["After"] footnotes[footnote[paragraph["Long\nnote."]] footnote[paragraph["The " Italic["first"] " defined."]]]]"#,
		);
	}

	#[test]
	fn numbers() {
		let compiled = compile_text("x[^b] y[^a] z[^b]\n\n[^a]: A\n[^b]: B\n");
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		let kinds: Vec<_> = (document.descendants(document.root()))
			.filter_map(|(_, element)| match &element.kind {
				nml_document::ElementKind::FootnoteRef {
					name,
					number,
					occurrence,
				} => Some(format!("ref {name} {number} {occurrence}")),
				nml_document::ElementKind::Footnote {
					name,
					number,
					references,
				} => Some(format!("note {name} {number} {references}")),
				_ => None,
			})
			.collect();
		assert_eq!(
			kinds,
			[
				"ref b 1 1",
				"ref a 2 1",
				"ref b 1 2",
				"note b 1 2",
				"note a 2 1"
			]
		);
	}

	#[test]
	fn errors() {
		let compiled = compile_text("x[^nope] y[^a]\n\n[^a]: A\n[^a]: Again\n[^unused]: U\n");
		assert_diagnostics!(
			compiled.diagnostics,
			[2031 @ 26..27, 2030 @ 38..44, 2029 @ 1..8]
		);
	}
}
