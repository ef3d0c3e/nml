//! Diagnostics: the central `DiagnosticKind` enum, located messages, fixes and Fluent
//! localization.

// The derive refers to this crate as `::nml_diagnostics`, also from inside it.
extern crate self as nml_diagnostics;

mod argument;
#[cfg(test)]
mod checks;
mod diagnostic;
mod kind;
mod localizer;
mod terminal;
#[doc(hidden)]
pub mod testing;

pub use argument::Argument;
pub use diagnostic::Diagnostic;
pub use diagnostic::DiagnosticMessage;
pub use diagnostic::DiagnosticSeverity;
pub use diagnostic::Edit;
pub use diagnostic::Fix;
pub use diagnostic::MessageContent;
pub use kind::DiagnosticKind;
pub use kind::KindInfo;
pub use localizer::FALLBACK_LOCALE;
pub use localizer::LocalizedDiagnostic;
pub use localizer::Localizer;
pub use terminal::render;
