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

/// `# Title`, … `###### Title`, with an optional ` {#ref}` that exports the heading.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let rule = Rule::new(
		"heading",
		vec![
			Atom::LineStart,
			Atom::capture("level", Atom::run('#', 1)).with_class("operator"),
			Atom::literal(" "),
			Atom::capture("title", Atom::InlineContent),
			Atom::optional(vec![
				Atom::literal(" {#"),
				Atom::capture("ref", Atom::Word).with_class("variable"),
				Atom::literal("}"),
			]),
			Atom::LineEnd,
		],
	);
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
		let reference = m.capture("ref").map(|capture| capture.token.clone());
		let attributes = Attributes {
			id: reference.as_ref().map(|r| r.content().to_string()),
			classes: Vec::new(),
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
