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
	#[diag(id = 2011, message = "diagnostic-second-reference-name")]
	SecondReferenceName { name: String },
	#[diag(id = 2012, message = "diagnostic-unknown-code-language", severity = warning)]
	UnknownCodeLanguage { language: String },
	#[diag(id = 2013, message = "diagnostic-unknown-media-type")]
	UnknownMediaType { url: String },
	#[diag(id = 2014, message = "diagnostic-unknown-callout")]
	UnknownCallout { kind: String, kinds: String },
	#[diag(id = 2015, message = "diagnostic-undefined-variable")]
	UndefinedVariable { name: String },
	#[diag(id = 2016, message = "diagnostic-variable-cycle")]
	VariableCycle { name: String },
	#[diag(id = 2017, message = "diagnostic-unknown-layout")]
	UnknownLayout { name: String, layouts: String },
	#[diag(id = 2018, message = "diagnostic-layout-parts")]
	LayoutParts {
		name: String,
		expected: usize,
		found: usize,
	},
	#[diag(id = 2019, message = "diagnostic-not-a-table")]
	NotATable,
	#[diag(id = 2020, message = "diagnostic-fence-language")]
	FenceLanguage { language: String, expected: String },
	#[diag(id = 2021, message = "diagnostic-invalid-output-path")]
	InvalidOutputPath { path: String },
	#[diag(id = 2022, message = "diagnostic-duplicate-table-name")]
	DuplicateTableName { name: String },
	/// A `:run` block whose fence does not name its runner.
	#[diag(id = 2023, message = "diagnostic-missing-runner")]
	MissingRunner,
	#[diag(id = 2024, message = "diagnostic-duplicate-run-name")]
	DuplicateRunName { name: String },
	/// `run:name` in a URL, without an earlier run of that name.
	#[diag(id = 2025, message = "diagnostic-unknown-run")]
	UnknownRun { name: String },
	#[diag(id = 2026, message = "diagnostic-missing-run-output")]
	MissingRunOutput { name: String, file: String },
	/// `run:name` for a run that did not write exactly one file; `files` lists those it
	/// wrote.
	#[diag(id = 2027, message = "diagnostic-ambiguous-run-output")]
	AmbiguousRunOutput {
		name: String,
		count: usize,
		files: String,
	},
	#[diag(id = 2003, message = "diagnostic-invalid-reference")]
	InvalidReference { target: String },
	#[diag(id = 2004, message = "diagnostic-invalid-variable-name")]
	InvalidVariableName { name: String },
	#[diag(id = 2005, message = "diagnostic-unknown-variable")]
	UnknownVariable { name: String },
	#[diag(id = 2006, message = "diagnostic-variable-already-set")]
	VariableAlreadySet { name: String },
	#[diag(id = 2007, message = "diagnostic-invalid-reference-key")]
	InvalidReferenceKey { key: String },
	#[diag(id = 2008, message = "diagnostic-import-outside-prologue")]
	ImportOutsidePrologue { module: String },
	#[diag(id = 2009, message = "diagnostic-duplicate-import")]
	DuplicateImport { namespace: String },
	#[diag(id = 2010, message = "diagnostic-invalid-module-path")]
	InvalidModulePath { module: String },

	#[diag(id = 3001, message = "diagnostic-unresolved-reference")]
	UnresolvedReference { target: String },
	#[diag(id = 3006, message = "diagnostic-ambiguous-reference")]
	AmbiguousReference { target: String, count: usize },
	#[diag(id = 3007, message = "diagnostic-unknown-module")]
	UnknownModule { name: String },
	#[diag(id = 3008, message = "diagnostic-file-not-found")]
	FileNotFound { path: String },
	#[diag(id = 3009, message = "diagnostic-include-cycle")]
	IncludeCycle { path: String },
	#[diag(id = 3010, message = "diagnostic-import-cycle")]
	ImportCycle { path: String },
	/// An error in `nml.toml`, with the TOML parser's own text.
	#[diag(id = 3002, message = "diagnostic-config-error")]
	ConfigError { message: String },
	#[diag(id = 3003, message = "diagnostic-invalid-glob")]
	InvalidGlob { pattern: String, message: String },
	#[diag(id = 3004, message = "diagnostic-invalid-module-name")]
	InvalidModuleName { name: String },
	#[diag(id = 3005, message = "diagnostic-duplicate-module")]
	DuplicateModule { name: String },
	/// An external renderer failed: the message is its own, untranslated.
	#[diag(id = 3012, message = "diagnostic-render-error")]
	RenderError { message: String },
	/// Two pages are written to the same file of the output; `other` is the other page.
	#[diag(id = 3013, message = "diagnostic-output-conflict")]
	OutputConflict { output: String, other: String },
	/// A file of the static directory would be written where the build writes its own
	/// files.
	#[diag(id = 3014, message = "diagnostic-reserved-static-file")]
	ReservedStaticFile { path: String },
	#[diag(id = 3015, message = "diagnostic-missing-static-directory")]
	MissingStaticDirectory { path: String },
	/// A `:run` block's runner is not in the configuration; `runners` lists those that are.
	#[diag(id = 3016, message = "diagnostic-unknown-runner")]
	UnknownRunner { runner: String, runners: String },
	/// A `:run` block failed: the message is its program's own, untranslated.
	#[diag(id = 3017, message = "diagnostic-run-failed")]
	RunFailed { message: String },
	#[diag(id = 3011, message = "diagnostic-unknown-code-theme")]
	UnknownCodeTheme { theme: String, themes: String },

	/// A Lua error, forwarded with Lua's own text.
	#[diag(id = 4001, message = "diagnostic-lua-error")]
	LuaError { message: String },
	/// A `:lua` block whose fence names another language.
	#[diag(id = 4002, message = "diagnostic-lua-fence-language")]
	LuaFenceLanguage { language: String },
	#[diag(id = 4003, message = "diagnostic-lua-rule-outside-prologue")]
	LuaRuleOutsidePrologue { rule: String },
	/// `message` is the grammar's reason, in English.
	#[diag(id = 4004, message = "diagnostic-invalid-lua-rule")]
	InvalidLuaRule { rule: String, message: String },
	#[diag(id = 4005, message = "diagnostic-deferred-module-not-imported")]
	DeferredModuleNotImported { module: String },
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
