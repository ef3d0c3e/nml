use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::BulletStyle;
use nml_document::Checkbox;
use nml_document::ElementKind;
use nml_document::ListKind;
use nml_document::NumberStyle;
use nml_grammar::Atom;
use nml_grammar::OptionKind;
use nml_grammar::OptionSchema;
use nml_grammar::OptionSpec;
use nml_grammar::OptionValue;
use nml_grammar::Options;
use nml_grammar::Prefix;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::RuleMatch;
use nml_grammar::Value;

use crate::Language;
use crate::Lower;

/// List entries: a line that starts with blanks, then markers, `*` for an unnumbered list
/// and `-` for a numbered one, then blanks and the entry. The run of markers is the depth:
/// ` ** b` is in a list inside the list of ` * a`, and ` *- c` in a numbered list inside it.
///
/// Options come right after the markers: `offset` sets the number of the entry (or the
/// start of a list it opens) and `bullet` the style of a list it opens. A checkbox follows,
/// after a blank: `[ ]`, `[-]` or `[x]`. The next lines that start with a blank, up to the
/// next entry, continue the entry.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let schema = Arc::new(OptionSchema::new([
		OptionSpec::new("offset", OptionKind::Integer)
			.with_default(OptionValue::Integer(1))
			.with_doc("The number of the entry, or the first number of the list it starts."),
		OptionSpec::new("bullet", OptionKind::Text)
			.with_default(OptionValue::Text(String::new()))
			.with_doc(
				"The style of the list the entry starts: `1`, `a`, `A`, `i` or `I` for a \
				numbered list, `disc`, `circle`, `square` or `none` otherwise.",
			),
	]));
	for (name, marker, doc, snippet) in [
		(
			"bullet_entry",
			"*",
			"**List entry**: ` * entry`, a blank, `*`, a blank and the entry.\n\n\
			```nml\n * Fruits\n ** Apple\n *- First step\n```\n\n\
			The run of markers is the depth: `**` is in a list inside the entry above, and \
			`*-` in a numbered list inside it. A checkbox follows: ` * [x] Done` (`[ ]`, \
			`[-]` or `[x]`). Options, right after the markers: `offset` (the entry's number), \
			`bullet` (`disc`, `circle`, `square`, `none`).",
			" * ${0}",
		),
		(
			"numbered_entry",
			"-",
			"**Numbered list entry**: ` - entry`, a blank, `-`, a blank and the entry.\n\n\
			```nml\n - First\n - Second\n -* A point of the second\n```\n\n\
			The run of markers is the depth. A checkbox follows: ` - [x] Done`. Options, right \
			after the markers: `offset` (the entry's number, or where the list starts), \
			`bullet` (`1`, `a`, `A`, `i`, `I`).",
			" - ${0}",
		),
	] {
		let markers = || Atom::or([vec![Atom::literal("*")], vec![Atom::literal("-")]]);
		let checkboxes =
			[" [ ]", " []", " [-]", " [x]", " [X]"].map(|checkbox| vec![Atom::literal(checkbox)]);
		let rule = Rule::new(
			name,
			vec![
				Atom::LineStart,
				Atom::Blanks,
				Atom::capture("marker", Atom::literal(marker)).with_class("operator"),
				Atom::capture("markers", Atom::repeat(vec![markers()], 0, None))
					.with_class("operator"),
				Atom::optional(vec![Atom::capture(
					"options",
					Atom::Options(schema.clone()),
				)]),
				Atom::optional(vec![
					Atom::capture("checkbox", Atom::or(checkboxes)).with_class("keyword"),
				]),
				Atom::Blanks,
				Atom::capture("body", Atom::Container(Prefix::Blanks)),
			],
		)
		// Without a blank after its markers, ` *word*` is no entry.
		.guarded(7)
		.with_doc(doc)
		.with_snippet(snippet);
		language.add_grouped(rule, |_| "list".into(), list)?;
	}

	// Progress cookies: how many of the item's or heading's tasks are done.
	for (name, literal, percent) in [
		("progress", "[/]", false),
		("progress_percent", "[%]", true),
	] {
		let rule = Rule::new(
			name,
			vec![Atom::capture("cookie", Atom::literal(literal)).with_class("keyword")],
		)
		.with_doc(
			"**Progress**: how many tasks of the list item or heading it is in are done, as \
			`[2/5]` for `[/]` or `[40%]` for `[%]`. An item counts its subitems with a \
			checkbox; a heading, the items with a checkbox in its section.",
		);
		language.add(rule, move |lower, m| {
			let inside = lower.builder.open_kinds().any(|kind| {
				matches!(
					kind,
					ElementKind::ListItem { .. } | ElementKind::Heading { .. }
				)
			});
			if !inside {
				let message = DiagnosticMessage::new(m.token.clone());
				let diagnostic = Diagnostic::new(DiagnosticKind::ProgressOutside, message);
				lower.diagnostics.push(diagnostic);
			}
			lower
				.builder
				.leaf(ElementKind::Progress { percent }, m.token.clone());
		})?;
	}
	Ok(())
}

/// A list open while its entries are lowered.
struct Level {
	numbered: bool,
	/// Whether an item of the list is open: deeper lists go in it.
	item_open: bool,
}

/// Lowers consecutive entries into nested lists.
fn list(lower: &mut Lower<'_, '_>, entries: &[&RuleMatch]) {
	let mut stack: Vec<Level> = Vec::new();
	for entry in entries {
		let markers = format!(
			"{}{}",
			entry.capture("marker").unwrap().token.content(),
			entry.capture("markers").unwrap().token.content()
		);
		let kinds: Vec<bool> = markers.chars().map(|c| c == '-').collect();
		let options = match entry.capture("options").map(|capture| &capture.value) {
			Some(Value::Options(options)) => Some(options),
			_ => None,
		};
		let offset = given(options, "offset").and_then(|value| match value {
			OptionValue::Integer(offset) => u64::try_from(*offset).ok(),
			_ => None,
		});
		let checkbox =
			entry
				.capture("checkbox")
				.map(|checkbox| match checkbox.token.content().trim() {
					"[-]" => Checkbox::Partial,
					"[x]" | "[X]" => Checkbox::Checked,
					_ => Checkbox::Unchecked,
				});

		// The lists the entry shares with the open ones.
		let common = stack
			.iter()
			.zip(&kinds)
			.take_while(|(level, numbered)| level.numbered == **numbered)
			.count();
		while stack.len() > common {
			close(lower, stack.pop().unwrap());
		}

		let mut value = None;
		if stack.len() == kinds.len() {
			// An entry of the innermost open list.
			let level = stack.last_mut().unwrap();
			if level.item_open {
				lower.builder.close();
			}
			if level.numbered {
				value = offset;
			}
		} else {
			for depth in stack.len()..kinds.len() {
				let numbered = kinds[depth];
				let last = depth + 1 == kinds.len();
				let kind = match last {
					true => list_kind(lower, numbered, options, offset),
					false => list_kind(lower, numbered, None, None),
				};
				lower
					.builder
					.open(ElementKind::List(kind), entry.token.clone());
				// A list deeper than the one after it, ` ** a` first, gets an empty item.
				if !last {
					let item = ElementKind::ListItem {
						checkbox: None,
						value: None,
					};
					lower.builder.open(item, entry.token.clone());
				}
				stack.push(Level {
					numbered,
					item_open: !last,
				});
			}
		}
		let item = ElementKind::ListItem { checkbox, value };
		lower.builder.open(item, entry.token.clone());
		stack.last_mut().unwrap().item_open = true;
		lower.capture(entry, "body");
	}
	while let Some(level) = stack.pop() {
		close(lower, level);
	}
}

fn close(lower: &mut Lower<'_, '_>, level: Level) {
	if level.item_open {
		lower.builder.close();
	}
	lower.builder.close();
}

/// An option written in the list, not its default.
fn given<'a>(options: Option<&'a Options>, name: &str) -> Option<&'a OptionValue> {
	let options = options?;
	options
		.entries
		.iter()
		.any(|entry| entry.key.content() == name)
		.then(|| options.get(name))
		.flatten()
}

/// The kind of a list an entry opens, with the entry's options.
fn list_kind(
	lower: &mut Lower<'_, '_>,
	numbered: bool,
	options: Option<&Options>,
	offset: Option<u64>,
) -> ListKind {
	let bullet = match given(options, "bullet") {
		Some(OptionValue::Text(bullet)) => Some(bullet.as_str()),
		_ => None,
	};
	let invalid = |lower: &mut Lower<'_, '_>, expected: Vec<&str>| {
		let entry = options
			.unwrap()
			.entries
			.iter()
			.find(|entry| entry.key.content() == "bullet")
			.unwrap();
		let location = entry
			.value_token
			.clone()
			.unwrap_or_else(|| entry.key.clone());
		let kind = DiagnosticKind::InvalidOptionValue {
			name: "bullet".into(),
			expected: expected.join(", "),
		};
		lower
			.diagnostics
			.push(Diagnostic::new(kind, DiagnosticMessage::new(location)));
	};
	if numbered {
		let style = bullet.and_then(|bullet| {
			let style = NumberStyle::from_name(bullet);
			if style.is_none() {
				invalid(lower, NumberStyle::ALL.iter().map(|s| s.name()).collect());
			}
			style
		});
		ListKind::Numbered {
			start: offset.unwrap_or(1),
			style,
		}
	} else {
		let style = bullet.and_then(|bullet| {
			let style = BulletStyle::from_name(bullet);
			if style.is_none() {
				invalid(lower, BulletStyle::ALL.iter().map(|s| s.name()).collect());
			}
			style
		});
		ListKind::Bullet { style }
	}
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::BulletStyle;
	use nml_document::Checkbox;
	use nml_document::ElementKind;
	use nml_document::ListKind;
	use nml_document::NumberStyle;

	use crate::tests::check;
	use crate::tests::compile_text;

	/// The kinds of the lists and items, in document order.
	fn kinds(text: &str) -> Vec<ElementKind> {
		let compiled = compile_text(text);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		document
			.descendants(document.root())
			.filter(|(_, element)| {
				matches!(
					element.kind,
					ElementKind::List(_) | ElementKind::ListItem { .. }
				)
			})
			.map(|(_, element)| element.kind.clone())
			.collect()
	}

	fn bullets() -> ElementKind {
		ElementKind::List(ListKind::Bullet { style: None })
	}

	fn numbered(start: u64) -> ElementKind {
		ElementKind::List(ListKind::Numbered { start, style: None })
	}

	fn item(checkbox: Option<Checkbox>, value: Option<u64>) -> ElementKind {
		ElementKind::ListItem { checkbox, value }
	}

	#[test]
	fn lists() {
		let compiled = check(
			" * a\n * **b**\n   more\n\n * c\n\ntext",
			r#"document[list[list_item[paragraph["a"]] list_item[paragraph[Bold["b"] "\nmore"]] list_item[paragraph["c"]]] paragraph["text"]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert_eq!(
			kinds("\t- a\n  - b"),
			[numbered(1), item(None, None), item(None, None)]
		);
	}

	#[test]
	fn nesting_by_markers() {
		check(
			" * a\n ** b\n *- c\n *- d\n * e",
			r#"document[list[list_item[paragraph["a"] list[list_item[paragraph["b"]]] list[list_item[paragraph["c"]] list_item[paragraph["d"]]]] list_item[paragraph["e"]]]]"#,
		);
		// A deeper first entry opens the lists above it with an empty item.
		check(
			" -* a\n - b",
			r#"document[list[list_item[list[list_item[paragraph["a"]]]] list_item[paragraph["b"]]]]"#,
		);
		// Another kind at the top starts another list.
		check(
			" * a\n - b",
			r#"document[list[list_item[paragraph["a"]]] list[list_item[paragraph["b"]]]]"#,
		);
	}

	#[test]
	fn entries_need_leading_and_trailing_blanks() {
		check(
			"a\n -5 degrees, \\*b",
			r#"document[paragraph["a\n -5 degrees, *b"]]"#,
		);
		let compiled = compile_text("* a\n - b");
		assert_diagnostics!(compiled.diagnostics, [1001]);
	}

	#[test]
	fn options() {
		assert_eq!(
			kinds(" -[offset=3] a\n - b\n -[offset=10] c"),
			[
				numbered(3),
				item(None, None),
				item(None, None),
				item(None, Some(10))
			]
		);
		assert_eq!(
			kinds(" -[bullet=i] a\n *[bullet=square] b\n ** c"),
			[
				ElementKind::List(ListKind::Numbered {
					start: 1,
					style: Some(NumberStyle::LowerRoman)
				}),
				item(None, None),
				ElementKind::List(ListKind::Bullet {
					style: Some(BulletStyle::Square)
				}),
				item(None, None),
				bullets(),
				item(None, None),
			]
		);
		let compiled = compile_text(" -[bullet=square] a");
		assert_diagnostics!(compiled.diagnostics, [1007 @ 10..16]);
		let compiled = compile_text(" *[bullet=I] a");
		assert_diagnostics!(compiled.diagnostics, [1007 @ 10..11]);
		let compiled = compile_text(" *[nope=1] a");
		assert_diagnostics!(compiled.diagnostics, [1005]);
	}

	#[test]
	fn checkboxes() {
		assert_eq!(
			kinds(" * [ ] a\n * [] b\n * [-] c\n *[offset=2] [x] d\n * [X] e\n * \\[y] f"),
			[
				bullets(),
				item(Some(Checkbox::Unchecked), None),
				item(Some(Checkbox::Unchecked), None),
				item(Some(Checkbox::Partial), None),
				item(Some(Checkbox::Checked), None),
				item(Some(Checkbox::Checked), None),
				item(None, None),
			]
		);
	}

	#[test]
	fn entries_hold_blocks() {
		check(
			" * a\n   ```\n   code\n   ```\n * b",
			r#"document[list[list_item[paragraph["a"] code_block[]] list_item[paragraph["b"]]]]"#,
		);
	}
}

#[cfg(test)]
mod progress_tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::ElementKind;

	use crate::tests::compile_text;

	/// The progress of each cookie, in order: `(done, total, percent)`.
	fn progress(text: &str) -> Vec<(usize, usize, bool)> {
		let compiled = compile_text(text);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		(document.descendants(document.root()))
			.filter_map(|(id, element)| match element.kind {
				ElementKind::Progress { percent } => {
					let (done, total) = document.progress(id).unwrap();
					Some((done, total, percent))
				}
				_ => None,
			})
			.collect()
	}

	#[test]
	fn items_count_their_subitems() {
		assert_eq!(
			progress(
				" * Release [/]\n ** [x] Changelog\n ** [ ] Tag [%]\n *** [x] a\n *** [-] b\n *** [ ] c\n \
				** [-] Announce\n ** Not a task\n * [x] Other [/]\n"
			),
			// Partly done counts as not done; deeper items are their item's.
			[(1, 3, false), (1, 3, true), (0, 0, false)]
		);
	}

	#[test]
	fn headings_count_their_section() {
		assert_eq!(
			progress(
				"# Plan [%]\n\n * [x] a\n * [ ] b\n\n## Part [/]\n\n * [x] c\n ** [x] d\n\n\
				# Other [/]\n\n * [ ] e\n"
			),
			// A section holds its subsections, at any depth.
			[(3, 4, true), (2, 2, false), (0, 1, false)]
		);
	}

	#[test]
	fn outside_of_tasks() {
		let compiled = compile_text("Done [/] here.\n");
		assert_diagnostics!(compiled.diagnostics, [2028 @ 5..8]);
	}
}
