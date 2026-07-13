use nml_document::Checkbox;
use nml_document::ElementKind;
use nml_document::ListKind;
use nml_grammar::Atom;
use nml_grammar::Prefix;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::RuleMatch;
use nml_source::Token;

use crate::Language;
use crate::Lower;

/// Bullet items `- text` and numbered items `1. text`, with an optional checkbox (`[ ]`,
/// `[-]`, `[x]`). An item's next lines are indented to the end of its marker. Consecutive
/// items of the same kind form a list; a numbered list starts at its first item's number.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let checkbox = || {
		let states = ["[ ] ", "[-] ", "[x] ", "[X] "].map(|state| vec![Atom::literal(state)]);
		Atom::optional(vec![
			Atom::capture("checkbox", Atom::or(states)).with_class("keyword"),
		])
	};
	let body = || Atom::capture("body", Atom::Container(Prefix::Indent));
	let bullet = Rule::new(
		"bullet_item",
		vec![
			Atom::LineStart,
			Atom::capture("marker", Atom::literal("- ")).with_class("operator"),
			checkbox(),
			body(),
		],
	);
	let numbered = Rule::new(
		"numbered_item",
		vec![
			Atom::LineStart,
			Atom::capture(
				"number",
				Atom::Number {
					signed: false,
					decimal: false,
				},
			)
			.with_class("operator"),
			Atom::capture("marker", Atom::literal(". ")).with_class("operator"),
			checkbox(),
			body(),
		],
	);
	language.add_grouped(bullet, |_| "bullet".into(), list)?;
	language.add_grouped(numbered, |_| "numbered".into(), list)?;
	Ok(())
}

/// Lowers consecutive items of one kind into a list.
fn list(lower: &mut Lower<'_, '_>, items: &[&RuleMatch]) {
	let first = &items[0].token;
	let last = &items[items.len() - 1].token;
	let token = Token::new(first.source().clone(), first.start()..last.end());
	let kind = match items[0].capture("number") {
		// Too many digits for a start number: count from the largest one.
		Some(number) => ListKind::Numbered {
			start: number.token.content().parse().unwrap_or(u64::MAX),
		},
		None => ListKind::Bullet,
	};
	lower.builder.open(ElementKind::List(kind), token);
	for item in items {
		let checkbox =
			item.capture("checkbox")
				.map(|checkbox| match &checkbox.token.content()[1..2] {
					" " => Checkbox::Unchecked,
					"-" => Checkbox::Partial,
					_ => Checkbox::Checked,
				});
		lower
			.builder
			.open(ElementKind::ListItem { checkbox }, item.token.clone());
		lower.capture(item, "body");
		lower.builder.close();
	}
	lower.builder.close();
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::Checkbox;
	use nml_document::ElementKind;
	use nml_document::ListKind;

	use crate::tests::check;

	#[test]
	fn bullet_lists() {
		let compiled = check(
			"- a\n- **b**\n  c\n\n- d\n\ntext",
			r#"document[list[list_item[paragraph["a"]] list_item[paragraph[Bold["b"] "\nc"]] list_item[paragraph["d"]]] paragraph["text"]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
	}

	#[test]
	fn numbered_lists_start_at_their_first_number() {
		let compiled = check(
			"3. a\n1. b\n10. c\n    d",
			r#"document[list[list_item[paragraph["a"]] list_item[paragraph["b"]] list_item[paragraph["c\nd"]]]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		let list = document.get(document.root()).children()[0];
		assert_eq!(
			document.get(list).kind,
			ElementKind::List(ListKind::Numbered { start: 3 })
		);

		// More digits than a start number holds.
		let compiled = check(
			"99999999999999999999999. a",
			r#"document[list[list_item[paragraph["a"]]]]"#,
		);
		let document = &compiled.unit.document;
		let list = document.get(document.root()).children()[0];
		assert_eq!(
			document.get(list).kind,
			ElementKind::List(ListKind::Numbered { start: u64::MAX })
		);
	}

	#[test]
	fn kinds_do_not_mix() {
		check(
			"- a\n1. b\n- c",
			r#"document[list[list_item[paragraph["a"]]] list[list_item[paragraph["b"]]] list[list_item[paragraph["c"]]]]"#,
		);
		// Anything between items ends the list.
		check(
			"- a\n\nb\n\n- c",
			r#"document[list[list_item[paragraph["a"]]] paragraph["b"] list[list_item[paragraph["c"]]]]"#,
		);
	}

	#[test]
	fn nested_lists() {
		let compiled = check(
			"- a\n  1. b\n  2. c\n     - d\n- e",
			r#"document[list[list_item[paragraph["a"] list[list_item[paragraph["b"]] list_item[paragraph["c"] list[list_item[paragraph["d"]]]]]] list_item[paragraph["e"]]]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
	}

	#[test]
	fn checkboxes() {
		let compiled = check(
			"- [ ] a\n- [-] b\n  more\n- [x] c\n- [X] d\n- \\[y] e\n- [link](x)",
			r#"document[list[list_item[paragraph["a"]] list_item[paragraph["b\nmore"]] list_item[paragraph["c"]] list_item[paragraph["d"]] list_item[paragraph["[y] e"]] list_item[paragraph[link["link"]]]]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		let list = document.get(document.root()).children()[0];
		let states: Vec<_> = document
			.children(list)
			.map(|(_, item)| match item.kind {
				ElementKind::ListItem { checkbox } => checkbox,
				_ => unreachable!(),
			})
			.collect();
		assert_eq!(
			states,
			[
				Some(Checkbox::Unchecked),
				Some(Checkbox::Partial),
				Some(Checkbox::Checked),
				Some(Checkbox::Checked),
				None,
				None
			]
		);
	}

	#[test]
	fn markers_need_a_space() {
		let compiled = check(
			"-a\n2024 was\n1.5",
			r#"document[paragraph["-a\n2024 was\n1.5"]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
	}
}
