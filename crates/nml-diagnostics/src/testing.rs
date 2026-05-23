//! Support for [`assert_diagnostics!`](crate::assert_diagnostics).

use std::fmt::Write;
use std::ops::Range;

use crate::Diagnostic;

/// One expected diagnostic: an ID and, optionally, the primary location's range.
pub type Expected = (u32, Option<Range<usize>>);

/// Panics unless `diagnostics` match `expected`, in order.
#[track_caller]
pub fn check(diagnostics: &[Diagnostic], expected: &[Expected]) {
	let matches = diagnostics.len() == expected.len()
		&& diagnostics
			.iter()
			.zip(expected)
			.all(|(diagnostic, (id, range))| {
				diagnostic.kind.id() == *id
					&& range
						.as_ref()
						.is_none_or(|range| diagnostic.primary().location.range() == *range)
			});
	if matches {
		return;
	}

	let mut message = String::from("diagnostics do not match\nexpected:\n");
	for (id, range) in expected {
		match range {
			Some(range) => writeln!(message, "  NML{id:04} @ {range:?}"),
			None => writeln!(message, "  NML{id:04}"),
		}
		.unwrap();
	}
	message.push_str("found:\n");
	for diagnostic in diagnostics {
		writeln!(
			message,
			"  {} @ {:?}  {:?}",
			diagnostic.kind.code(),
			diagnostic.primary().location.range(),
			diagnostic.kind
		)
		.unwrap();
	}
	panic!("{message}");
}

/// Asserts that a list of diagnostics has the given IDs, in order, optionally with the range
/// of each primary location.
///
/// ```
/// # use std::sync::Arc;
/// # use nml_diagnostics::{assert_diagnostics, Diagnostic, DiagnosticKind, DiagnosticMessage};
/// # use nml_source::{Source, Token, Url};
/// # let source = Arc::new(Source::new(Url::parse("file:///a.nml").unwrap(), "a **b".into(), 0));
/// let kind = DiagnosticKind::ExpectedToken { token: "**".into() };
/// let diagnostics = vec![Diagnostic::new(kind, DiagnosticMessage::new(Token::new(source, 2..4)))];
/// assert_diagnostics!(diagnostics, [1001 @ 2..4]);
/// assert_diagnostics!(diagnostics, [1001]);
/// ```
#[macro_export]
macro_rules! assert_diagnostics {
	($diagnostics:expr, [$($id:literal $(@ $range:expr)?),* $(,)?]) => {
		$crate::testing::check(
			&$diagnostics,
			&[$(($id, $crate::assert_diagnostics!(@range $($range)?))),*],
		)
	};
	(@range) => { None };
	(@range $range:expr) => { Some($range) };
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use nml_source::Source;
	use nml_source::Token;
	use nml_source::Url;

	use crate::DiagnosticKind;
	use crate::DiagnosticMessage;

	use super::*;

	fn diagnostics() -> Vec<Diagnostic> {
		let source = Arc::new(Source::new(
			Url::parse("file:///a.nml").unwrap(),
			"a **b".into(),
			0,
		));
		vec![
			Diagnostic::new(
				DiagnosticKind::ExpectedToken { token: "**".into() },
				DiagnosticMessage::new(Token::new(source.clone(), 2..4)),
			),
			Diagnostic::new(
				DiagnosticKind::UnexpectedToken { token: "b".into() },
				DiagnosticMessage::new(Token::new(source, 4..5)),
			),
		]
	}

	#[test]
	fn matches() {
		assert_diagnostics!(diagnostics(), [1001 @ 2..4, 1002]);
		assert_diagnostics!(Vec::<Diagnostic>::new(), []);
	}

	#[test]
	#[should_panic(expected = "NML1002 @ 4..5")]
	fn wrong_id() {
		assert_diagnostics!(diagnostics(), [1001, 1001]);
	}

	#[test]
	#[should_panic(expected = "diagnostics do not match")]
	fn wrong_range() {
		assert_diagnostics!(diagnostics(), [1001 @ 2..3, 1002]);
	}

	#[test]
	#[should_panic(expected = "diagnostics do not match")]
	fn wrong_count() {
		assert_diagnostics!(diagnostics(), [1001]);
	}
}
