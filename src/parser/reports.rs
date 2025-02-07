use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use ariadne::Color;
use dashmap::DashMap;
use tower_lsp::lsp_types::Diagnostic;

use crate::parser::source::LineCursor;

use super::source::OffsetEncoding;
use super::source::Source;
use super::source::SourcePosition;
use super::source::Token;

/// Store the different colors used for diagnostics.
/// Colors have to be set to `None` for the language server.
#[derive(Debug, Clone)]
pub struct ReportColors {
	pub error: Option<Color>,
	pub warning: Option<Color>,
	pub info: Option<Color>,
	pub highlight: Option<Color>,
}

impl ReportColors {
	pub fn with_colors() -> Self {
		Self {
			error: Some(Color::Red),
			warning: Some(Color::Yellow),
			info: Some(Color::BrightBlue),
			highlight: Some(Color::BrightMagenta),
		}
	}

	pub fn without_colors() -> Self {
		Self {
			error: None,
			warning: None,
			info: None,
			highlight: None,
		}
	}
}

#[derive(Debug)]
pub enum ReportKind {
	Error,
	Warning,
}

impl From<&ReportKind> for ariadne::ReportKind<'static> {
	fn from(val: &ReportKind) -> Self {
		match val {
			ReportKind::Error => ariadne::ReportKind::Error,
			ReportKind::Warning => ariadne::ReportKind::Warning,
		}
	}
}

impl From<&ReportKind> for tower_lsp::lsp_types::DiagnosticSeverity {
	fn from(val: &ReportKind) -> Self {
		match val {
			ReportKind::Error => tower_lsp::lsp_types::DiagnosticSeverity::ERROR,
			ReportKind::Warning => tower_lsp::lsp_types::DiagnosticSeverity::WARNING,
		}
	}
}

#[derive(Debug)]
pub struct ReportSpan {
	pub token: Token,
	pub message: String,
}

#[derive(Debug)]
pub struct Report {
	pub kind: ReportKind,
	pub source: Arc<dyn Source>,
	pub message: String,
	pub note: Option<String>,
	pub help: Option<String>,
	pub spans: Vec<ReportSpan>,
}

impl Report {
	fn ariadne_color(kind: &ReportKind, colors: &ReportColors) -> ariadne::Color {
		match kind {
			ReportKind::Error => colors.error.unwrap_or(ariadne::Color::Primary),
			ReportKind::Warning => colors.warning.unwrap_or(ariadne::Color::Primary),
		}
	}

	fn to_ariadne(
		self,
		colors: &ReportColors,
	) -> (
		ariadne::Report<'static, (Arc<dyn Source>, Range<usize>)>,
		impl ariadne::Cache<Arc<dyn Source>>,
	) {
		let mut cache = HashMap::new();
		let source = self.source.original_position(0).0;
		let mut start = usize::MAX;
		for span in &self.spans {
			let (osource, opos) = span.token.source().original_position(span.token.start());

			if osource == source.clone() && opos < start {
				start = opos;
			}
		}
		if start == usize::MAX {
			start = 0;
		}
		cache.insert(source.clone(), source.content().clone());
		let mut builder = ariadne::Report::build((&self.kind).into(), self.source, start)
			.with_message(self.message);

		for span in self.spans {
			cache.insert(span.token.source(), span.token.source().content().clone());
			builder = builder.with_label(
				ariadne::Label::new(span.token.source().original_range(span.token.range))
					.with_message(span.message)
					.with_color(Self::ariadne_color(&self.kind, colors)),
			)
		}
		if let Some(help) = &self.help {
			builder.set_help(help);
		}
		if let Some(note) = &self.note {
			builder.set_note(note);
		}

		(builder.finish(), ariadne::sources(cache))
	}

	pub fn reports_to_stdout(colors: &ReportColors, mut reports: Vec<Report>) {
		reports.drain(..).for_each(|report| {
			let (report, cache) = report.to_ariadne(colors);
			report.eprint(cache).unwrap();
		});
	}

	fn to_diagnostics(self, diagnostic_map: &DashMap<String, Vec<Diagnostic>>) {
		for span in self.spans {
			let (source, range) = span.token.source().original_range(span.token.range.clone());

			let mut start = LineCursor::new(source.clone(), OffsetEncoding::Utf16);
			start.move_to(range.start);
			let mut end = start.clone();
			end.move_to(range.end);

			let diag = Diagnostic {
				range: tower_lsp::lsp_types::Range {
					start: tower_lsp::lsp_types::Position {
						line: start.line as u32,
						character: start.line_pos as u32,
					},
					end: tower_lsp::lsp_types::Position {
						line: end.line as u32,
						character: end.line_pos as u32,
					},
				},
				severity: Some((&self.kind).into()),
				code: None,
				code_description: None,
				source: None,
				message: format!("{}: {}", self.message, span.message),
				related_information: None,
				tags: None,
				data: None,
			};
			if let Some(mut diags) = diagnostic_map.get_mut(source.name()) {
				diags.push(diag);
			} else {
				diagnostic_map.insert(source.name().to_owned(), vec![diag]);
			}
		}
	}

	pub fn reports_to_diagnostics(
		diagnostic_map: &DashMap<String, Vec<Diagnostic>>,
		mut reports: Vec<Report>,
	) {
		for report in reports.drain(..) {
			report.to_diagnostics(diagnostic_map);
		}
		//diagnostics
	}
}

pub mod macros {

	#[macro_export]
	macro_rules! report_label {
		($r:expr,) => {{ }};
		($r:expr, span($source:expr, $range:expr, $message:expr) $(, $($tail:tt)*)?) => {{
			$r.spans.push(ReportSpan {
				token: $crate::parser::source::Token::new($range, $source),
				message: $message,
			});
			report_label!($r, $($($tail)*)?);
		}};
		($r:expr, span($range:expr, $message:expr) $(, $($tail:tt)*)?) => {{
			$r.spans.push(ReportSpan {
				token: $crate::parser::source::Token::new($range, $r.source.clone()),
				message: $message,
			});
			report_label!($r, $($($tail)*)?);
		}};
		($r:expr, note($message:expr) $(, $($tail:tt)*)?) => {{
			$r.note = Some($message);
			report_label!($r, $($($tail)*)?);
		}};
		($r:expr, help($message:expr) $(, $($tail:tt)*)?) => {{
			$r.help = Some($message);
			report_label!($r, $($($tail)*)?);
		}}
	}

	#[macro_export]
	macro_rules! make_err {
		($source:expr, $message:expr, $($tail:tt)*) => {{
			let mut r = Report {
				kind: ReportKind::Error,
				source: $source,
				message: $message,
				note: None,
				help: None,
				spans: vec![],
			};
			report_label!(r, $($tail)*);
			r
		}}
	}

	#[macro_export]
	macro_rules! compile_err {
		($token:expr, $message:expr, $explanation:expr) => {{
			let mut r = Report {
				kind: ReportKind::Error,
				source: $token.source(),
				message: $message,
				note: None,
				help: None,
				spans: vec![],
			};
			report_label!(r, span($token.range.clone(), $explanation));
			vec![r]
		}};
	}

	#[macro_export]
	macro_rules! report_err {
		($unit:expr, $source:expr, $message:expr, $($tail:tt)*) => {{
			let mut r = Report {
				kind: ReportKind::Error,
				source: $source,
				message: $message,
				note: None,
				help: None,
				spans: vec![],
			};
			report_label!(r, $($tail)*);
			$unit.report(r);
		}}
	}

	#[macro_export]
	macro_rules! report_warn {
		($unit:expr, $source:expr, $message:expr, $($tail:tt)*) => {{
			let mut r = Report {
				kind: ReportKind::Warning,
				source: $source,
				message: $message,
				note: None,
				help: None,
				spans: vec![],
			};
			report_label!(r, $($tail)*);
			$unit.report(r);
		}}
	}

	pub use crate::*;
}
