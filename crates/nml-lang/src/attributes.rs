//! Attributes in braces, `{#ref .class}`: a reference name and HTML classes, after a heading
//! or a medium.

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::Attributes;
use nml_grammar::Atom;
use nml_grammar::RuleMatch;
use nml_source::Token;

use crate::Lower;

/// Optional attributes in braces, after `open` (which ends with `{`): `#ref` and `.class`,
/// separated by spaces.
pub(crate) fn atoms(open: &str) -> Atom {
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
	Atom::optional(vec![
		attribute(open),
		Atom::repeat(vec![attribute(" ")], 0, None),
		Atom::literal("}"),
	])
}

/// The attributes of a match of [`atoms`], and its reference name. A second name is
/// reported; a class given twice is kept once.
pub(crate) fn lower(lower: &mut Lower<'_, '_>, m: &RuleMatch) -> (Attributes, Option<Token>) {
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
	(attributes, reference)
}
