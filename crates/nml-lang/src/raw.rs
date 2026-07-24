use nml_document::ElementKind;
use nml_grammar::Atom;
use nml_grammar::Rule;
use nml_grammar::RuleError;

use crate::Language;

/// Raw output, written without escaping: the role ``:raw`<br>` `` and the command `:raw`
/// with a fenced body.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let block = Rule::new(
		"raw",
		vec![
			Atom::LineStart,
			Atom::capture("command", Atom::literal(":raw")).with_class("keyword"),
			Atom::LineEnd,
			Atom::LineStart,
			Atom::capture("fence", Atom::run('`', 3)).with_class("operator"),
			Atom::LineEnd,
			Atom::capture("content", Atom::Text),
			Atom::LineStart,
			Atom::capture("close", Atom::same("fence")).with_class("operator"),
			Atom::LineEnd,
		],
	);
	language.add(block, |lower, m| {
		let content = m.capture("content").unwrap().token.content();
		let content = content.strip_suffix('\n').unwrap_or(content);
		lower.builder.leaf(raw(content, true), m.token.clone());
	})?;

	let role = Rule::new(
		"raw_role",
		vec![
			Atom::capture("command", Atom::literal(":raw")).with_class("keyword"),
			Atom::capture("open", Atom::run('`', 1)).with_class("operator"),
			Atom::capture("content", Atom::Text),
			Atom::capture("close", Atom::same("open")).with_class("operator"),
		],
	);
	language.add(role, |lower, m| {
		let content = m.capture("content").unwrap().token.content();
		lower.builder.leaf(raw(content, false), m.token.clone());
	})?;
	Ok(())
}

fn raw(content: &str, block: bool) -> ElementKind {
	ElementKind::Raw {
		content: content.into(),
		block,
	}
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::ElementKind;

	use crate::tests::check;
	use crate::tests::compile_text;

	fn raws(text: &str) -> Vec<ElementKind> {
		let compiled = compile_text(text);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		document
			.descendants(document.root())
			.filter(|(_, element)| matches!(element.kind, ElementKind::Raw { .. }))
			.map(|(_, element)| element.kind.clone())
			.collect()
	}

	#[test]
	fn raw_output() {
		check(
			"a :raw`<br>` b\n:raw\n```\n<hr>\n```\nc",
			r#"document[paragraph["a " raw[] " b"] raw[] paragraph["c"]]"#,
		);
		assert_eq!(raws(":raw`` a`b ``"), [super::raw(" a`b ", false)]);
		assert_eq!(
			raws(":raw\n````\n```\n<p>**x**</p>\n````"),
			[super::raw("```\n<p>**x**</p>", true)]
		);
		// No fence: `:raw` alone is not a command.
		let compiled = compile_text(":raw\ntext");
		assert_diagnostics!(compiled.diagnostics, [1003]);
	}
}
