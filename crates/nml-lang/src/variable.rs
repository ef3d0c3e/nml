use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::Scope;
use nml_document::SymbolRef;
use nml_grammar::Atom;
use nml_grammar::Rule;
use nml_grammar::RuleError;

use crate::Language;

/// The variable that sets a unit's reference key.
pub(crate) const REFERENCE_KEY: &str = "nml.reference_key";

/// The variables NML itself reads; other `nml.` names are errors.
const BUILTIN: &[&str] = &[REFERENCE_KEY];

/// `:set name = value`. For now, variables are only stored: substitution comes later.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let rule = Rule::new(
		"set",
		vec![
			Atom::LineStart,
			Atom::capture("command", Atom::literal(":set ")).with_class("keyword"),
			Atom::capture("name", Atom::Text).with_class("variable"),
			Atom::literal(" = "),
			Atom::capture("value", Atom::RestOfLine).with_class("string"),
			Atom::LineEnd,
		],
	);
	language.add(rule, |lower, m| {
		let name = m.capture("name").unwrap().token.clone();
		let value = m.capture("value").unwrap().token.clone();
		let error = |kind| Diagnostic::new(kind, DiagnosticMessage::new(name.clone()));
		let text = name.content();
		if !is_variable_name(text) {
			let kind = DiagnosticKind::InvalidVariableName { name: text.into() };
			lower.diagnostics.push(error(kind));
			return;
		}
		if text.starts_with("nml.") && !BUILTIN.contains(&text) {
			let kind = DiagnosticKind::UnknownVariable { name: text.into() };
			lower.diagnostics.push(error(kind));
			return;
		}
		if let Some((_, first)) = lower.variables.get(text) {
			let kind = DiagnosticKind::VariableAlreadySet { name: text.into() };
			let diagnostic = error(kind).with(DiagnosticMessage::fluent(first.clone(), "first"));
			lower.diagnostics.push(diagnostic);
			return;
		}
		if text == REFERENCE_KEY
			&& nml_document::SymbolRef::parse(value.content())
				.is_none_or(|r| r.scope != nml_document::Scope::Local)
		{
			let kind = DiagnosticKind::InvalidReferenceKey {
				key: value.content().into(),
			};
			lower
				.diagnostics
				.push(Diagnostic::new(kind, DiagnosticMessage::new(value)));
			return;
		}
		lower
			.variables
			.insert(text.into(), (value.content().into(), name));
	})?;
	Ok(())
}

/// Dot-separated words: `nml.reference_key`, `title`.
fn is_variable_name(name: &str) -> bool {
	name.split('.').all(|part| {
		let mut chars = part.chars();
		chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
			&& chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-')
	})
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;

	use crate::tests::compile_text;

	#[test]
	fn reference_key() {
		let compiled =
			compile_text(":set nml.reference_key = setup\n:set title = Install NML\n# A");
		assert_diagnostics!(compiled.diagnostics, []);
		assert_eq!(compiled.unit.reference_key.as_deref(), Some("setup"));

		let compiled = compile_text("# A");
		assert_eq!(compiled.unit.reference_key, None);
	}

	#[test]
	fn errors() {
		for (text, expected) in [
			(":set nml.colour = x", vec![(2005, 5..15)]),
			(":set 1x = y", vec![(2004, 5..7)]),
			(":set a..b = y", vec![(2004, 5..9)]),
			(":set nml.reference_key = a b", vec![(2007, 25..28)]),
			(":set nml.reference_key = a#b", vec![(2007, 25..28)]),
			(":set x = 1\n:set x = 2", vec![(2006, 16..17)]),
		] {
			let compiled = compile_text(text);
			let expected: Vec<_> = expected.into_iter().map(|(id, r)| (id, Some(r))).collect();
			nml_diagnostics::testing::check(&compiled.diagnostics, &expected);
		}
		// A missing ` = ` leaves the name unclosed.
		let compiled = compile_text(":set x");
		assert_diagnostics!(compiled.diagnostics, [1001 @ 0..5]);
	}
}
