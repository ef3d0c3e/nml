use nml_document::Attributes;
use nml_document::ElementKind;
use nml_document::Export;
use nml_document::SymbolKind;
use nml_grammar::Atom;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::Value;

use crate::Language;
use crate::language::plain_text;

/// `:anchor name [Title]`: a reference target where it is written. Its title is the
/// `Title`, or the name.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let rule = Rule::new(
		"anchor",
		vec![
			Atom::LineStart,
			Atom::capture("command", Atom::literal(":anchor ")).with_class("keyword"),
			Atom::capture("ref", Atom::Word).with_class("variable"),
			Atom::optional(vec![
				Atom::literal(" "),
				Atom::capture("title", Atom::InlineContent),
			]),
			Atom::LineEnd,
		],
	);
	language.add(rule, |lower, m| {
		let reference = m.capture("ref").unwrap().token.clone();
		let title = match m.capture("title").map(|capture| &capture.value) {
			Some(Value::Nodes(nodes)) => plain_text(nodes),
			_ => reference.content().to_string(),
		};
		let attributes = Attributes {
			id: Some(reference.content().to_string()),
			classes: Vec::new(),
		};
		lower
			.builder
			.open_with(ElementKind::Anchor, m.token.clone(), attributes);
		let element = lower.builder.close();
		let export = Export::new(reference.content(), SymbolKind::Anchor, Some(element))
			.with("title", title);
		lower.export(export, reference);
	})?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;

	use crate::tests::check;
	use crate::tests::compile_text;

	#[test]
	fn anchors() {
		let compiled = check(
			"a\n:anchor setup The *setup*\nb\n:anchor end\nSee [[setup]].",
			r#"document[paragraph["a"] anchor#setup[] paragraph["b"] anchor#end[] paragraph["See " reference[slot[]] "."]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let titles: Vec<_> = compiled
			.unit
			.exports
			.iter()
			.map(|export| export.property("title").unwrap())
			.collect();
		assert_eq!(titles, ["The setup", "end"]);

		let compiled = compile_text(":anchor a\n# A {#a}");
		assert_diagnostics!(compiled.diagnostics, [2002 @ 16..17]);
	}
}
