use std::io;
use std::io::Write;

use nml_diagnostics::Diagnostic;
use nml_document::OutputContext;

use crate::Template;
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

/// Writes a page by streaming a template and the value of each of its slots: the slots'
/// positions in `content` and contexts, in order of position. `resolve` gives the value of
/// the slot at an index of `slots`. This is the whole of a relink.
///
/// Returns the diagnostics of the slots that could not be filled.
pub fn write(
	content: &str,
	slots: &[(usize, OutputContext)],
	mut resolve: impl FnMut(usize) -> Resolved,
	out: &mut impl Write,
) -> io::Result<Vec<Diagnostic>> {
	let mut diagnostics = Vec::new();
	let mut written = 0;
	let mut value = String::new();
	for (index, (position, context)) in slots.iter().enumerate() {
		out.write_all(&content.as_bytes()[written..*position])?;
		written = *position;
		value.clear();
		match resolve(index) {
			Resolved::Value(text) => match context {
				OutputContext::Text => escape(&mut value, &text, false),
				OutputContext::Attribute => escape(&mut value, &text, true),
				OutputContext::Markup => value = text,
			},
			Resolved::Missing(diagnostic) => {
				diagnostics.push(diagnostic);
				if *context != OutputContext::Attribute {
					value.push_str(FALLBACK);
				}
			}
		}
		out.write_all(value.as_bytes())?;
	}
	out.write_all(&content.as_bytes()[written..])?;
	Ok(diagnostics)
}

impl Template {
	/// The positions and contexts of the slots, for [`write`].
	pub fn slot_positions(&self) -> Vec<(usize, OutputContext)> {
		self.slots
			.iter()
			.map(|slot| (slot.position, slot.context))
			.collect()
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use nml_diagnostics::DiagnosticKind;
	use nml_diagnostics::DiagnosticMessage;
	use nml_source::Source;
	use nml_source::Token;
	use nml_source::Url;

	use super::*;

	#[test]
	fn fills_slots_in_their_context() {
		let slots = [
			(9, OutputContext::Attribute),
			(11, OutputContext::Text),
			(12, OutputContext::Markup),
		];
		let mut out = Vec::new();
		let diagnostics = write(
			"<a href=\"\">!</a>",
			&slots,
			|index| {
				Resolved::Value(match index {
					0 => "a.html#\"x\"".into(),
					1 => "<T>".into(),
					_ => "<b>m</b>".into(),
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
		let slots = [(9, OutputContext::Attribute), (11, OutputContext::Text)];
		let mut out = Vec::new();
		let diagnostics = write(
			"<a href=\"\"></a>",
			&slots,
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

	#[test]
	fn no_slots() {
		let mut out = Vec::new();
		write("<p>x</p>", &[], |_| unreachable!(), &mut out).unwrap();
		assert_eq!(out, b"<p>x</p>");
	}
}
