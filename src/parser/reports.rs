use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use super::parser::ReportColors;
use super::source::Source;
use super::source::SourcePosition;
use super::source::Token;

#[derive(Debug)]
pub enum ReportKind {
	Error,
	Warning,
}

impl Into<ariadne::ReportKind<'static>> for &ReportKind {
	fn into(self) -> ariadne::ReportKind<'static> {
		match self {
			ReportKind::Error => ariadne::ReportKind::Error,
			ReportKind::Warning => ariadne::ReportKind::Warning,
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
	pub source: Rc<dyn Source>,
	pub message: String,
	pub note: Option<String>,
	pub help: Option<String>,
	pub spans: Vec<ReportSpan>,
}

impl Report {
	fn ariadne_color(kind: &ReportKind, colors: &ReportColors) -> ariadne::Color {
		match kind {
			ReportKind::Error => colors.error,
			ReportKind::Warning => colors.warning,
		}
	}

	pub fn to_ariadne(
		self,
		colors: &ReportColors,
	) -> (
		ariadne::Report<'static, (Rc<dyn Source>, Range<usize>)>,
		impl ariadne::Cache<Rc<dyn Source>>,
	) {
		let mut cache = HashMap::new();
		let source = self.source.original_position(0).0;
		let mut start = usize::MAX;
		for span in &self.spans {
			let (osource, opos) = span.token.source().original_position(span.token.start());

			if &osource == &source && opos < start {
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
}

pub mod macros {
	pub use super::*;
	#[macro_export]
	macro_rules! report_label {
		($r:expr,) => {{ }};
		($r:expr, span($source:expr, $range:expr, $message:expr) $(, $($tail:tt)*)?) => {{
			$r.spans.push(ReportSpan {
				token: crate::parser::source::Token::new($range, $source),
				message: $message,
			});
			report_label!($r, $($($tail)*)?);
		}};
		($r:expr, span($range:expr, $message:expr) $(, $($tail:tt)*)?) => {{
			$r.spans.push(ReportSpan {
				token: crate::parser::source::Token::new($range, $r.source.clone()),
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
	macro_rules! report_err {
		($reports:expr, $source:expr, $message:expr, $($tail:tt)*) => {{
			let mut r = Report {
				kind: ReportKind::Error,
				source: $source,
				message: $message,
				note: None,
				help: None,
				spans: vec![],
			};
			report_label!(r, $($tail)*);
			$reports.push(r);
		}}
	}

	#[macro_export]
	macro_rules! report_warn {
		($reports:expr, $source:expr, $message:expr, $($tail:tt)*) => {{
			let mut r = Report {
				kind: ReportKind::Warning,
				source: $source,
				message: $message,
				note: None,
				help: None,
				spans: vec![],
			};
			report_label!(r, $($tail)*);
			$reports.push(r);
		}}
	}

	pub use crate::*;
}
