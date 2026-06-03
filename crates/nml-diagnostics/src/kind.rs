use nml_diagnostics_macros::Diagnostic;

use crate::DiagnosticSeverity;

/// Every kind of diagnostic NML can report.
///
/// IDs are stable and never reused. They are grouped by area:
/// - 1xxx: grammar engine
/// - 2xxx: built-in language
/// - 3xxx: workspace, build and imports
/// - 4xxx: Lua
/// - 5xxx: renderers
/// - 6xxx: HTML output
///
/// `message` is the Fluent message ID; each field is a Fluent argument of the same name.
///
/// The derive rejects a reused ID:
/// ```compile_fail
/// #[derive(nml_diagnostics_macros::Diagnostic)]
/// enum Kind {
///     #[diag(id = 1, message = "a")]
///     A,
///     #[diag(id = 1, message = "b")]
///     B,
/// }
/// ```
/// and a variant without its ID or message:
/// ```compile_fail
/// #[derive(nml_diagnostics_macros::Diagnostic)]
/// enum Kind {
///     #[diag(message = "a")]
///     A,
/// }
/// ```
/// while a well-formed enum compiles:
/// ```
/// #[derive(nml_diagnostics_macros::Diagnostic)]
/// enum Kind {
///     #[diag(id = 1, message = "a", severity = warning)]
///     A { name: String },
///     #[diag(id = 2, message = "b")]
///     B,
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Diagnostic)]
pub enum DiagnosticKind {
	#[diag(id = 1001, message = "diagnostic-expected-token")]
	ExpectedToken { token: String },
	#[diag(id = 1002, message = "diagnostic-unexpected-token")]
	UnexpectedToken { token: String },
	/// An atom of a rule did not match; `atom` names it (`Word`, `LineEnd`, …).
	#[diag(id = 1003, message = "diagnostic-expected-atom")]
	ExpectedAtom { atom: String },
	#[diag(id = 1004, message = "diagnostic-nesting-too-deep")]
	NestingTooDeep { limit: usize },
	#[diag(id = 1005, message = "diagnostic-unknown-option")]
	UnknownOption { name: String },
	#[diag(id = 1006, message = "diagnostic-duplicate-option")]
	DuplicateOption { name: String },
	/// `expected` is the option's kind: `Text`, `Integer`, `Number`, `Bool`, or the list of
	/// choices.
	#[diag(id = 1007, message = "diagnostic-invalid-option-value")]
	InvalidOptionValue { name: String, expected: String },
	#[diag(id = 1008, message = "diagnostic-missing-option")]
	MissingOption { name: String },

	#[diag(id = 2001, message = "diagnostic-heading-too-deep")]
	HeadingTooDeep { level: usize, max: usize },
	#[diag(id = 2002, message = "diagnostic-duplicate-reference")]
	DuplicateReference { name: String },

	/// A Lua error, forwarded with Lua's own text.
	#[diag(id = 4001, message = "diagnostic-lua-error")]
	LuaError { message: String },
}

/// Static description of a [`DiagnosticKind`] variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindInfo {
	pub id: u32,
	pub name: &'static str,
	pub message_id: &'static str,
	pub severity: DiagnosticSeverity,
	/// Field names, which are also the Fluent argument names.
	pub fields: &'static [&'static str],
}

impl DiagnosticKind {
	/// The user-facing code, e.g. `NML1001`.
	pub fn code(&self) -> String {
		format!("NML{:04}", self.id())
	}
}

#[cfg(test)]
mod tests {
	use std::collections::HashSet;

	use crate::Argument;

	use super::*;

	#[test]
	fn metadata() {
		let kind = DiagnosticKind::ExpectedToken { token: "**".into() };
		assert_eq!(kind.id(), 1001);
		assert_eq!(kind.code(), "NML1001");
		assert_eq!(kind.message_id(), "diagnostic-expected-token");
		assert_eq!(kind.default_severity(), DiagnosticSeverity::Error);
		assert_eq!(kind.args(), vec![("token", Argument::from("**"))]);
	}

	#[test]
	fn kinds_list_every_variant() {
		let ids: HashSet<_> = DiagnosticKind::KINDS.iter().map(|kind| kind.id).collect();
		assert_eq!(ids.len(), DiagnosticKind::KINDS.len());
		let lua = DiagnosticKind::KINDS
			.iter()
			.find(|kind| kind.name == "LuaError")
			.unwrap();
		assert_eq!(lua.id, 4001);
		assert_eq!(lua.fields, &["message"]);
	}
}
