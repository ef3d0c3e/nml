use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::Export;
use nml_document::Scope;
use nml_document::SymbolKind;
use nml_document::SymbolRef;
use nml_grammar::Atom;
use nml_grammar::Node;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::RuleMatch;
use nml_source::Url;
use nml_source::VirtualSourceBuilder;

use crate::Language;
use crate::Lower;
use crate::Variable;
use crate::language::plain_text;

/// The variable that sets a unit's reference key.
pub(crate) const REFERENCE_KEY: &str = "nml.reference_key";

/// The variables NML itself reads; other `nml.` names are errors.
const BUILTIN: &[&str] = &[REFERENCE_KEY];

/// `:set name = value` and `:export name = value`, and `%name%`, which stands for the value
/// of a variable, parsed as inline markup where it is substituted.
///
/// An exported variable is also a symbol of the unit (its title is the value's text), and
/// the units that import it as a module see it as `%namespace.name%`.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	for (command, export) in [(":set ", false), (":export ", true)] {
		let rule = Rule::new(
			command.trim_start_matches(':').trim_end(),
			vec![
				Atom::LineStart,
				Atom::capture("command", Atom::literal(command)).with_class("keyword"),
				Atom::capture("name", Atom::Text).with_class("variable"),
				Atom::literal(" = "),
				Atom::capture("value", Atom::RestOfLine).with_class("string"),
				Atom::LineEnd,
			],
		);
		language.add_prologue(rule, move |lower, m| set(lower, m, export))?;
	}

	let substitution = Rule::new(
		"substitution",
		vec![
			Atom::capture("open", Atom::literal("%")).with_class("operator"),
			Atom::capture("name", Atom::Text).with_class("variable"),
			Atom::capture("close", Atom::literal("%")).with_class("operator"),
		],
	);
	language.add(substitution, substitute)?;

	language.on_import(|lower, import| {
		// The module's `:export` lines, at the top of the file.
		let rules = lower.language().rules().clone();
		let parsed = nml_grammar::parse(&rules, import.source.clone());
		for node in &parsed.nodes {
			let Node::Match(m) = node else { continue };
			if !m.complete || rules.get(m.rule).is_none_or(|rule| rule.name != "export") {
				continue;
			}
			let name = m.capture("name").unwrap().token.clone();
			let value = m.capture("value").unwrap().token.clone();
			if is_variable_name(name.content()) {
				let key = format!("{}.{}", import.namespace, name.content());
				lower.variables.insert(key, Variable { name, value });
			}
		}
	});
	Ok(())
}

fn set(lower: &mut Lower<'_, '_>, m: &RuleMatch, export: bool) {
	let name = m.capture("name").unwrap().token.clone();
	let value = m.capture("value").unwrap().token.clone();
	let error = |kind| Diagnostic::new(kind, DiagnosticMessage::new(name.clone()));
	let text = name.content();
	if !is_variable_name(text) {
		let kind = DiagnosticKind::InvalidVariableName { name: text.into() };
		lower.diagnostics.push(error(kind));
		return;
	}
	if text.starts_with("nml.") && (export || !BUILTIN.contains(&text)) {
		let kind = DiagnosticKind::UnknownVariable { name: text.into() };
		lower.diagnostics.push(error(kind));
		return;
	}
	if let Some(first) = lower.variables.get(text) {
		let kind = DiagnosticKind::VariableAlreadySet { name: text.into() };
		let first = DiagnosticMessage::fluent(first.name.clone(), "first");
		lower.diagnostics.push(error(kind).with(first));
		return;
	}
	if text == REFERENCE_KEY
		&& SymbolRef::parse(value.content()).is_none_or(|r| r.scope != Scope::Local)
	{
		let kind = DiagnosticKind::InvalidReferenceKey {
			key: value.content().into(),
		};
		lower
			.diagnostics
			.push(Diagnostic::new(kind, DiagnosticMessage::new(value)));
		return;
	}
	if export {
		let parsed = nml_grammar::parse_inline(lower.language().rules(), value_source(&value));
		let export = Export::new(text, SymbolKind::Variable, None)
			.with("title", plain_text(&parsed.nodes))
			.with("value", value.content());
		lower.export(export, name.clone());
	}
	lower
		.variables
		.insert(text.into(), Variable { name, value });
}

/// The variables being substituted, innermost last: a variable whose value refers to itself
/// would never end.
#[derive(Default)]
struct Substituting(Vec<String>);

fn substitute(lower: &mut Lower<'_, '_>, m: &RuleMatch) {
	let name = m.capture("name").unwrap().token.clone();
	let error = |kind| Diagnostic::new(kind, DiagnosticMessage::new(name.clone()));
	let Some(variable) = lower.variables.get(name.content()).cloned() else {
		let kind = DiagnosticKind::UndefinedVariable {
			name: name.content().into(),
		};
		lower.diagnostics.push(error(kind));
		return;
	};
	let substituting = lower.extension(Substituting::default);
	if substituting.0.iter().any(|n| n == name.content()) {
		let kind = DiagnosticKind::VariableCycle {
			name: name.content().into(),
		};
		lower.diagnostics.push(error(kind));
		return;
	}
	substituting.0.push(name.content().into());
	let parsed = nml_grammar::parse_inline(lower.language().rules(), value_source(&variable.value));
	lower.diagnostics.extend(parsed.diagnostics);
	lower.nodes(&parsed.nodes);
	lower.extension(Substituting::default).0.pop();
}

/// A variable's value as a source of its own, whose positions map to where it is written.
fn value_source(value: &nml_source::Token) -> Arc<nml_source::Source> {
	let (root, offset) = value
		.source()
		.root_offset(value.start(), nml_source::Bias::Start);
	let mut location = Url::parse("virtual:///").unwrap();
	location.set_path(&format!("{}/variable/{offset}", root.location().path()));
	let mut builder = VirtualSourceBuilder::new(value.source().clone(), location, value.start());
	builder.copy(value.range());
	Arc::new(builder.build())
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

	use crate::tests::check;
	use crate::tests::compile_text;
	use crate::tests::compile_with;

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
	fn substitution() {
		let compiled = check(
			":set version = **2.1**\n:set name = NML %version%\n\n# %name%\nUse %name%, 50\\%.",
			r#"document[h1["NML " Bold["2.1"]] paragraph["Use " "NML " Bold["2.1"] ", 50%."]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		// A variable is set where it is written: it can be set after the prologue.
		check(
			"a\n\n:set x = y\n%x%",
			r#"document[paragraph["a"] paragraph["y"]]"#,
		);
	}

	#[test]
	fn substitution_errors() {
		let compiled = compile_text("%nope% and %%");
		assert_diagnostics!(compiled.diagnostics, [2015 @ 1..5, 2015 @ 12..12]);
		// Errors in a value are reported where it is written.
		let compiled = compile_text(":set x = *a\n%x%");
		assert_diagnostics!(compiled.diagnostics, [1001]);
		let location = compiled.diagnostics[0].primary().location.root();
		assert_eq!(location.range(), 9..10);
		assert_eq!(location.source().location().as_str(), "file:///page.nml");
		let compiled = compile_text(":set a = %b%\n:set b = %a%\n%a%");
		assert_diagnostics!(compiled.diagnostics, [2016]);
		let location = compiled.diagnostics[0].primary().location.root();
		assert_eq!(location.range(), 23..24);
		// An unescaped `%` opens a substitution.
		let compiled = compile_text("50% off");
		assert_diagnostics!(compiled.diagnostics, [1001]);
	}

	#[test]
	fn exports() {
		let compiled = check(
			":export version = *2.1*\n%version%",
			r#"document[paragraph[Italic["2.1"]]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let export = &compiled.unit.exports[0];
		assert_eq!(export.name, "version");
		assert_eq!(export.property("title"), Some("2.1"));
		assert_eq!(export.property("value"), Some("*2.1*"));

		let compiled = compile_text(":export nml.reference_key = x");
		assert_diagnostics!(compiled.diagnostics, [2005]);
	}

	#[test]
	fn module_variables() {
		let compiled = compile_with(
			":import std.meta\n:import std.meta as m\n%meta.version% %m.version%",
			&[(
				"modules/std/meta.nml",
				":export version = **3**\n:set private = x\n",
			)],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		assert_eq!(
			crate::tests::show(document, document.root()),
			r#"document[paragraph[Bold["3"] " " Bold["3"]]]"#
		);
		let compiled = compile_with(
			":import std.meta\n%meta.private%",
			&[("modules/std/meta.nml", ":set private = x\n")],
		);
		assert_diagnostics!(compiled.diagnostics, [2015]);
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
