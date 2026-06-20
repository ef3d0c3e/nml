use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::ElementKind;
use nml_document::LinkSlot;
use nml_document::OutputContext;
use nml_document::Property;
use nml_document::Slot;
use nml_document::SymbolRef;
use nml_document::Use;
use nml_grammar::Atom;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::Value;

use crate::Language;

/// `[[name]]`, `[[doc#name]]`, `[[doc#]]` and `[[#name]]`, with an optional `|caption`.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let rule = Rule::new(
		"reference",
		vec![
			Atom::capture("open", Atom::literal("[[")).with_class("operator"),
			Atom::capture("target", Atom::Text).with_class("variable"),
			Atom::optional(vec![
				Atom::literal("|"),
				Atom::capture("caption", Atom::InlineContent),
			]),
			Atom::capture("close", Atom::literal("]]")).with_class("operator"),
		],
	);
	language.add(rule, |lower, m| {
		let target_token = m.capture("target").unwrap().token.clone();
		let caption = m.capture("caption").map(|capture| &capture.value);
		let Some(target) = SymbolRef::parse(target_token.content()) else {
			let kind = DiagnosticKind::InvalidReference {
				target: target_token.content().into(),
			};
			let message = DiagnosticMessage::new(target_token);
			lower.diagnostics.push(Diagnostic::new(kind, message));
			return;
		};
		lower.uses.push(Use {
			target: target.clone(),
			token: target_token.clone(),
		});

		let link = |property, context| {
			Slot::Link(LinkSlot {
				target: target.clone(),
				property,
				context,
			})
		};
		let href = lower
			.builder
			.add_slot(link(Property::Target, OutputContext::Attribute));
		lower
			.builder
			.open(ElementKind::Reference { href }, m.token.clone());
		match caption {
			Some(Value::Nodes(nodes)) => lower.nodes(nodes),
			_ => {
				let title = link(Property::Title, OutputContext::Text);
				lower.builder.slot(title, target_token);
			}
		}
		lower.builder.close();
	})?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::ElementKind;
	use nml_document::Slot;
	use nml_document::SymbolRef;

	use crate::tests::check;

	#[test]
	fn references() {
		let compiled = check(
			"See [[builds]], [[config#pages|the **page** rules]], [[guide#]] and [[#faq]].",
			r#"document[paragraph["See " reference[slot[]] ", " reference["the " Bold["page"] " rules"] ", " reference[slot[]] " and " reference[slot[]] "."]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);

		let targets: Vec<_> = compiled
			.unit
			.uses
			.iter()
			.map(|u| (u.target.clone(), u.token.content().to_string()))
			.collect();
		assert_eq!(
			targets,
			[
				(SymbolRef::local("builds"), "builds".into()),
				(SymbolRef::in_unit("config", "pages"), "config#pages".into()),
				(SymbolRef::page("guide"), "guide#".into()),
				(SymbolRef::any("faq"), "#faq".into()),
			]
		);

		// Each reference has a URL slot; those without a caption also show the title.
		let document = &compiled.unit.document;
		let slots: Vec<_> = document
			.slots()
			.map(|(_, slot)| match slot {
				Slot::Link(link) => {
					format!("{} {:?} {:?}", link.target, link.property, link.context)
				}
				_ => unreachable!(),
			})
			.collect();
		assert_eq!(
			slots,
			[
				"builds Target Attribute",
				"builds Title Text",
				"config#pages Target Attribute",
				"guide# Target Attribute",
				"guide# Title Text",
				"#faq Target Attribute",
				"#faq Title Text",
			]
		);
		let href = document
			.descendants(document.root())
			.find_map(|(_, e)| match e.kind {
				ElementKind::Reference { href } => Some(href),
				_ => None,
			})
			.unwrap();
		assert_eq!(document.slot(href), document.slots().next().unwrap().1);
	}

	#[test]
	fn invalid_references() {
		for (text, id, range) in [
			("[[a#b#c]]", 2003, 2..7),
			("[[a b]]", 2003, 2..5),
			("[[]]", 2003, 2..2),
			("[[#]]", 2003, 2..3),
			// Unclosed.
			("[[a", 1001, 0..2),
			("[[a|b", 1001, 0..2),
		] {
			let compiled = crate::tests::compile_text(text);
			nml_diagnostics::testing::check(&compiled.diagnostics, &[(id, Some(range))]);
			assert!(compiled.unit.uses.is_empty(), "{text:?}");
		}
	}
}
