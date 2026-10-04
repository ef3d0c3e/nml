use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::ElementKind;
use nml_document::LayoutKind;
use nml_grammar::Atom;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::Value;

use crate::Language;

/// Layouts: `:layout name [Title]`, then parts separated by `:next`, and `:end` (or
/// `:end layout`). The parts are parsed blocks.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let rule = Rule::new(
		"layout",
		vec![
			Atom::LineStart,
			Atom::capture("command", Atom::literal(":layout ")).with_class("keyword"),
			Atom::capture("name", Atom::Word).with_class("type"),
			Atom::optional(vec![
				Atom::literal(" "),
				Atom::capture("title", Atom::InlineContent),
			]),
			Atom::LineEnd,
			Atom::capture("part", Atom::Content),
			Atom::repeat(
				vec![
					Atom::LineStart,
					Atom::capture("next", Atom::literal(":next")).with_class("keyword"),
					Atom::LineEnd,
					Atom::capture("part", Atom::Content),
				],
				0,
				None,
			),
			Atom::LineStart,
			Atom::capture("end", Atom::literal(":end")).with_class("keyword"),
			Atom::optional(vec![
				Atom::capture("end_name", Atom::literal(" layout")).with_class("keyword"),
			]),
			Atom::LineEnd,
		],
	)
	.with_doc(
		"**Layout**: content arranged in parts, separated by `:next`.\n\n\
		```nml\n:layout split\nLeft.\n:next\nRight.\n:end\n```\n\n\
		Layouts: `centered` (one part), `split` (side by side, any number of parts), \
		`spoiler` (one part, hidden until opened; the title is what is shown), `sidebar` \
		(one part, beside the page's content when it is not inside another block).",
	)
	.with_snippet(":layout ${1|split,centered,spoiler,sidebar|}\n${2}\n:next\n${0}\n:end");
	language.add(rule, |lower, m| {
		let name = m.capture("name").unwrap().token.clone();
		let Some(kind) = LayoutKind::from_name(name.content()) else {
			let layouts: Vec<_> = LayoutKind::ALL.iter().map(|kind| kind.name()).collect();
			let kind = DiagnosticKind::UnknownLayout {
				name: name.content().into(),
				layouts: layouts.join(", "),
			};
			lower
				.diagnostics
				.push(Diagnostic::new(kind, DiagnosticMessage::new(name)));
			return;
		};
		let parts: Vec<_> = m.captures("part").collect();
		if let Some(expected) = kind.parts()
			&& parts.len() != expected
		{
			let kind = DiagnosticKind::LayoutParts {
				name: name.content().into(),
				expected,
				found: parts.len(),
			};
			// At the first `:next` too many.
			let location = m.captures("next").nth(expected - 1).unwrap().token.clone();
			lower
				.diagnostics
				.push(Diagnostic::new(kind, DiagnosticMessage::new(location)));
		}
		lower
			.builder
			.open(ElementKind::Layout(kind), m.token.clone());
		if let Some(title) = m.capture("title") {
			lower
				.builder
				.open(ElementKind::Caption, title.token.clone());
			lower.capture(m, "title");
			lower.builder.close();
		}
		for part in parts {
			lower
				.builder
				.open(ElementKind::LayoutPart, part.token.clone());
			if let Value::Nodes(nodes) = &part.value {
				lower.nodes(nodes);
			}
			lower.builder.close();
		}
		lower.builder.close();
	})?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;

	use crate::tests::check;
	use crate::tests::compile_text;

	#[test]
	fn layouts() {
		let compiled = check(
			":layout split\nThe source…\n:next\n * …and\n * the result.\n:end\nafter",
			r#"document[layout[layout_part[paragraph["The source…"]] layout_part[list[list_item[paragraph["…and"]] list_item[paragraph["the result."]]]]] paragraph["after"]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let compiled = check(
			":layout spoiler The *answer*\n42\n:end layout",
			r#"document[layout[caption["The " Italic["answer"]] layout_part[paragraph["42"]]]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		// Layouts nest: `:next` and `:end` belong to the innermost one.
		let compiled = check(
			":layout split\n:layout centered\na\n:end\n:next\nb\n:end",
			r#"document[layout[layout_part[layout[layout_part[paragraph["a"]]]] layout_part[paragraph["b"]]]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
	}

	#[test]
	fn errors() {
		let compiled = compile_text(":layout grid\na\n:end");
		assert_diagnostics!(compiled.diagnostics, [2017 @ 8..12]);
		let compiled = compile_text(":layout centered\na\n:next\nb\n:next\nc\n:end");
		assert_diagnostics!(compiled.diagnostics, [2018 @ 19..24]);
		let compiled = compile_text(":layout split\na\n");
		assert_diagnostics!(compiled.diagnostics, [1001]);
		let compiled = compile_text(":layout split\na\n:end lua");
		assert_diagnostics!(compiled.diagnostics, [1003]);
	}
}
