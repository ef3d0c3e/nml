//! Conversions between NML's types and the protocol's.

use std::str::FromStr;

use lsp_types::DiagnosticRelatedInformation;
use lsp_types::DiagnosticSeverity;
use lsp_types::Location;
use lsp_types::NumberOrString;
use lsp_types::Position;
use lsp_types::Range;
use lsp_types::Uri;
use nml_diagnostics::Diagnostic;
use nml_diagnostics::Localizer;
use nml_source::Encoding;
use nml_source::Token;
use nml_source::Url;

/// The range of a token in its file, in UTF-16 columns, as the protocol counts them.
/// Tokens in virtual sources are mapped to the file.
pub(crate) fn range(token: &Token) -> Range {
	let root = token.root();
	let source = root.source();
	let position = |offset| {
		let position = source
			.position(offset, Encoding::Utf16)
			.expect("a token's offsets are in its source");
		Position::new(position.line as u32, position.column as u32)
	};
	Range::new(position(root.start()), position(root.end()))
}

/// The file a diagnostic is reported in: where its primary location is, in the file.
pub(crate) fn file(diagnostic: &Diagnostic) -> Url {
	diagnostic
		.primary()
		.location
		.root()
		.source()
		.location()
		.clone()
}

fn uri(url: &Url) -> Uri {
	Uri::from_str(url.as_str()).expect("a URL is a URI")
}

/// A diagnostic for the protocol: its code, its localized title, and its secondary
/// messages as related information.
pub(crate) fn diagnostic(diagnostic: &Diagnostic, localizer: &Localizer) -> lsp_types::Diagnostic {
	let localized = localizer.localize(diagnostic);
	let severity = match diagnostic.severity {
		nml_diagnostics::DiagnosticSeverity::Error => DiagnosticSeverity::ERROR,
		nml_diagnostics::DiagnosticSeverity::Warning => DiagnosticSeverity::WARNING,
		nml_diagnostics::DiagnosticSeverity::Hint => DiagnosticSeverity::HINT,
	};
	let mut message = localized.title.clone();
	if let Some(Some(label)) = localized.labels.first() {
		message = format!("{message}: {label}");
	}
	let related = diagnostic
		.messages()
		.iter()
		.zip(&localized.labels)
		.skip(1)
		.map(|(located, label)| DiagnosticRelatedInformation {
			location: Location::new(
				uri(&located.location.root().source().location().clone()),
				range(&located.location),
			),
			message: label.clone().unwrap_or_else(|| localized.title.clone()),
		})
		.collect::<Vec<_>>();
	lsp_types::Diagnostic {
		range: range(&diagnostic.primary().location),
		severity: Some(severity),
		code: Some(NumberOrString::String(diagnostic.kind.code())),
		source: Some("nml".into()),
		message,
		related_information: (!related.is_empty()).then_some(related),
		..lsp_types::Diagnostic::default()
	}
}
