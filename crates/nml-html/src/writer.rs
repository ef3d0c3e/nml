use std::io;
use std::io::Write;

use nml_diagnostics::Diagnostic;
use nml_document::OutputContext;

use crate::Template;
use crate::TemplateSlot;
use crate::escape;

/// The value of a slot.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
	/// The value, unescaped: it is escaped for the slot's context.
	Value(String),
	/// The slot cannot be filled. The page shows a visible fallback.
	Missing(Diagnostic),
}

/// What an unfilled slot shows.
const FALLBACK: &str = "<span class=\"nml-unresolved\">??</span>";

/// Writes a page by streaming the template and the value of each slot. This is the whole of
/// a relink. Returns the diagnostics of the slots that could not be filled.
pub fn write(
	template: &Template,
	mut resolve: impl FnMut(&TemplateSlot) -> Resolved,
	out: &mut impl Write,
) -> io::Result<Vec<Diagnostic>> {
	let mut diagnostics = Vec::new();
	let mut written = 0;
	let mut value = String::new();
	for slot in &template.slots {
		out.write_all(&template.content.as_bytes()[written..slot.position])?;
		written = slot.position;
		value.clear();
		match resolve(slot) {
			Resolved::Value(text) => match slot.context {
				OutputContext::Text => escape(&mut value, &text, false),
				OutputContext::Attribute => escape(&mut value, &text, true),
				OutputContext::Markup => value = text,
			},
			Resolved::Missing(diagnostic) => {
				diagnostics.push(diagnostic);
				if slot.context != OutputContext::Attribute {
					value.push_str(FALLBACK);
				}
			}
		}
		out.write_all(value.as_bytes())?;
	}
	out.write_all(&template.content.as_bytes()[written..])?;
	Ok(diagnostics)
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use nml_diagnostics::DiagnosticKind;
	use nml_diagnostics::DiagnosticMessage;
	use nml_document::SlotId;
	use nml_source::Source;
	use nml_source::Token;
	use nml_source::Url;

	use super::*;

	fn slot(index: u32, position: usize, context: OutputContext) -> TemplateSlot {
		TemplateSlot {
			slot: slot_id(index),
			position,
			context,
		}
	}

	/// Slot IDs are only created by documents: the ID of the slot at `index` in one.
	fn slot_id(index: u32) -> SlotId {
		use nml_document::DocumentBuilder;
		use nml_document::LinkSlot;
		use nml_document::Property;
		use nml_document::Slot;
		use nml_document::SymbolRef;

		let source = Arc::new(Source::new(
			Url::parse("file:///a.nml").unwrap(),
			String::new(),
			0,
		));
		let mut builder = DocumentBuilder::new(source.clone());
		let mut id = None;
		for _ in 0..=index {
			id = Some(builder.slot(
				Slot::Link(LinkSlot {
					target: SymbolRef::local("x"),
					property: Property::Title,
					context: OutputContext::Text,
				}),
				Token::new(source.clone(), 0..0),
			));
		}
		id.unwrap()
	}

	#[test]
	fn fills_slots_in_their_context() {
		let template = Template {
			content: "<a href=\"\">!</a>".into(),
			slots: vec![
				slot(0, 9, OutputContext::Attribute),
				slot(1, 11, OutputContext::Text),
				slot(2, 12, OutputContext::Markup),
			],
		};
		let mut out = Vec::new();
		let diagnostics = write(
			&template,
			|slot| {
				Resolved::Value(match slot.context {
					OutputContext::Attribute => "a.html#\"x\"".into(),
					OutputContext::Text => "<T>".into(),
					OutputContext::Markup => "<b>m</b>".into(),
				})
			},
			&mut out,
		)
		.unwrap();
		assert!(diagnostics.is_empty());
		assert_eq!(
			String::from_utf8(out).unwrap(),
			"<a href=\"a.html#&quot;x&quot;\">&lt;T&gt;!<b>m</b></a>"
		);
	}

	#[test]
	fn missing_slots_show_a_fallback() {
		let source = Arc::new(Source::new(
			Url::parse("file:///a.nml").unwrap(),
			"[[x]]".into(),
			0,
		));
		let template = Template {
			content: "<a href=\"\"></a>".into(),
			slots: vec![
				slot(0, 9, OutputContext::Attribute),
				slot(1, 11, OutputContext::Text),
			],
		};
		let mut out = Vec::new();
		let diagnostics = write(
			&template,
			|_| {
				let kind = DiagnosticKind::UnresolvedReference {
					target: "#x".into(),
				};
				let location = DiagnosticMessage::new(Token::new(source.clone(), 0..5));
				Resolved::Missing(Diagnostic::new(kind, location))
			},
			&mut out,
		)
		.unwrap();
		assert_eq!(diagnostics.len(), 2);
		assert_eq!(
			String::from_utf8(out).unwrap(),
			format!("<a href=\"\">{FALLBACK}</a>")
		);
	}
}
