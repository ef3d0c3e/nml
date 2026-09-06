use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::Attributes;
use nml_document::ElementKind;
use nml_document::Export;
use nml_document::SymbolKind;
use nml_grammar::Atom;
use nml_grammar::Rule;
use nml_grammar::RuleError;

use crate::Language;
use crate::language::plain_text;

/// The deepest heading level, as in HTML.
const MAX_LEVEL: usize = 6;

/// `# Title`, … `###### Title`, with optional attributes in braces: `{#ref}` exports the
/// heading under `ref`, and `.class` adds a class (`.unnumbered`, `.unlisted`, or any HTML
/// class): `# Title {#ref .unlisted}`.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let attribute = |open: &str| {
		Atom::or([
			vec![
				Atom::literal(format!("{open}#")),
				Atom::capture("ref", Atom::Word).with_class("variable"),
			],
			vec![
				Atom::literal(format!("{open}.")),
				Atom::capture("class", Atom::Word).with_class("type"),
			],
		])
	};
	let rule = Rule::new(
		"heading",
		vec![
			Atom::LineStart,
			Atom::capture("level", Atom::run('#', 1)).with_class("operator"),
			Atom::literal(" "),
			Atom::capture("title", Atom::InlineContent),
			Atom::optional(vec![
				attribute(" {"),
				Atom::repeat(vec![attribute(" ")], 0, None),
				Atom::literal("}"),
			]),
			Atom::LineEnd,
		],
	)
	.with_doc(
		"**Heading**: `#` to `######` and a title, one `#` per level.\n\n\
		```nml\n## Installation {#install .unlisted}\n```\n\n\
		In braces, `#name` makes it referenceable as `[[name]]`, and `.class` gives it an HTML \
		class (`unlisted` keeps it out of the table of contents, `unnumbered` unnumbers it).",
	)
	.with_snippet("# ${1:Title}");
	language.add(rule, |lower, m| {
		let level_token = &m.capture("level").unwrap().token;
		let mut level = level_token.content().len();
		if level > MAX_LEVEL {
			let kind = DiagnosticKind::HeadingTooDeep {
				level,
				max: MAX_LEVEL,
			};
			let message = DiagnosticMessage::new(level_token.clone());
			lower.diagnostics.push(Diagnostic::new(kind, message));
			level = MAX_LEVEL;
		}
		let mut references = m.captures("ref").map(|capture| capture.token.clone());
		let reference = references.next();
		for second in references {
			let kind = DiagnosticKind::SecondReferenceName {
				name: second.content().into(),
			};
			let first = DiagnosticMessage::fluent(reference.clone().unwrap(), "first");
			let diagnostic = Diagnostic::new(kind, DiagnosticMessage::new(second)).with(first);
			lower.diagnostics.push(diagnostic);
		}
		let mut classes: Vec<String> = Vec::new();
		for class in m.captures("class") {
			let class = class.token.content();
			if !classes.iter().any(|c| c == class) {
				classes.push(class.into());
			}
		}
		let attributes = Attributes {
			id: reference.as_ref().map(|r| r.content().to_string()),
			classes,
		};

		let heading = ElementKind::Heading { level: level as u8 };
		lower
			.builder
			.open_with(heading, m.token.clone(), attributes);
		lower.capture(m, "title");
		let element = lower.builder.close();

		if let Some(reference) = reference {
			let title = match m.capture("title").map(|capture| &capture.value) {
				Some(nml_grammar::Value::Nodes(nodes)) => plain_text(nodes),
				_ => String::new(),
			};
			let export = Export::new(reference.content(), SymbolKind::Heading, Some(element))
				.with("title", title);
			lower.export(export, reference);
		}
	})?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;

	use crate::tests::check;

	#[test]
	fn headings() {
		let compiled = check(
			"# Title\n## Incremental **builds** {#builds}\ntext\n###### Deep",
			r#"document[h1["Title"] h2#builds["Incremental " Bold["builds"]] paragraph["text"] h6["Deep"]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let export = &compiled.unit.exports[0];
		assert_eq!(export.name, "builds");
		assert_eq!(export.property("title"), Some("Incremental builds"));
		let (file, range) = export.declared.clone().unwrap();
		assert_eq!(file, "page.nml");
		assert_eq!(&compiled.unit.document.source().content()[range], "builds");
	}

	#[test]
	fn attributes() {
		let compiled = check(
			"# A {.unnumbered .unlisted}\n## B {#b .x .unlisted .x}\n### C {.y #c}\n# D {x}",
			r#"document[h1["A"] h2#b["B"] h3#c["C"] h1["D {x}"]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		let classes: Vec<_> = document
			.children(document.root())
			.map(|(_, heading)| heading.attributes.classes.join(" "))
			.collect();
		assert_eq!(classes, ["unnumbered unlisted", "x unlisted", "y", ""]);
		assert_eq!(compiled.unit.exports.len(), 2);

		let compiled = compile_text("# A {#a #b}");
		assert_diagnostics!(compiled.diagnostics, [2011 @ 9..10]);
		let compiled = compile_text("# A {#a .}");
		assert_diagnostics!(compiled.diagnostics, [1003]);
		let compiled = compile_text("# A {#a}x");
		assert_diagnostics!(compiled.diagnostics, [1003]);
	}

	#[test]
	fn heading_errors() {
		let compiled = check("####### Too deep", r#"document[h6["Too deep"]]"#);
		assert_diagnostics!(compiled.diagnostics, [2001 @ 0..7]);

		let compiled = check("# A {#x}\n# B {#x}", r#"document[h1#x["A"] h1#x["B"]]"#);
		assert_diagnostics!(compiled.diagnostics, [2002 @ 15..16]);
		assert_eq!(compiled.unit.exports.len(), 1);

		// `#` at the start of a line is always a heading marker.
		let compiled = compile_text("#hashtag");
		assert_diagnostics!(compiled.diagnostics, [1001 @ 1..2]);
	}

	use crate::tests::compile_text;
}
