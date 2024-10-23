use std::{ops::Range, rc::Rc};

use super::{parser::Parser, source::{Source, SourcePosition, Token}};

#[derive(Debug)]
enum ReportKind
{
	Error,
	Warning,
}

impl Into<ariadne::ReportKind<'static>> for &ReportKind
{
    fn into(self) -> ariadne::ReportKind<'static> {
		match self
		{
			ReportKind::Error => ariadne::ReportKind::Error,
			ReportKind::Warning => ariadne::ReportKind::Warning,
		}
    }
}

#[derive(Debug)]
struct ReportSpan
{
	pub token: Token,
	pub message: String
}

#[derive(Debug)]
struct Report
{
	pub kind: ReportKind,
	pub source: Rc<dyn Source>,
	pub message: String,
	pub spans: Vec<ReportSpan>,
}

impl Report
{
	fn ariadne_format(fmt: &str, parser: &dyn Parser) -> String
	{
		// TODO: Colors
		return fmt.to_string();
	}

	fn ariadne_color(kind: &ReportKind, parser: &dyn Parser) -> ariadne::Color
	{
		match kind
		{
			ReportKind::Error => parser.colors().error,
			ReportKind::Warning => parser.colors().warning,
		}
	}

	pub fn to_ariadne(self, parser: &dyn Parser) -> ariadne::Report<'static, (Rc<dyn Source>, Range<usize>)>
	{
		let source = self.source.original_position(0).0;
		let mut start = usize::MAX;
		for span in &self.spans
		{
			let (osource, opos) = span.token.source().original_position(span.token.start());

			if &osource == &source && opos < start
			{
				start = opos;
			}
		}
		if start == usize::MAX
		{
			start = 0;
		}
		let mut builder = ariadne::Report::build((&self.kind).into(), self.source, start)
			.with_message(Self::ariadne_format(self.message.as_str(), parser));

		for span in self.spans
		{
			builder = builder.with_label(
				ariadne::Label::new((span.token.source(), span.token.range))
					.with_message(Self::ariadne_format(span.message.as_str(), parser))
					.with_color(Self::ariadne_color(&self.kind, parser))
				)
		}

		builder.finish()
	}
}

macro_rules! report_label {
	($spans:expr, $psource:expr,) => {{ }};
	($spans:expr, $psource:expr, span($source:expr, $range:expr, $message:expr), $(, $($tail:tt)*)?) => {{
		$spans.push(ReportSpan {
			token: Token::new($range, $source),
			message: $message,
		});
		report_label!($spans, $psource, $($($tail)*)?);
	}};
	($spans:expr, $psource:expr, span($range:expr, $message:expr) $(, $($tail:tt)*)?) => {{
		$spans.push(ReportSpan {
			token: Token::new($range, $psource),
			message: $message,
		});
		report_label!($spans, $psource, $($($tail)*)?);
	}}
}

#[macro_export]
macro_rules! report_err {
	($reports:expr, $source:expr, $message:expr, $($tail:tt)*) => {{
		let mut spans = Vec::new();
		report_label!(spans, $source.clone(), $($tail)*);
		$reports.push(Report {
			kind: ReportKind::Error,
			source: $source,
			message: $message,
			spans,
		});
	}}
}

#[cfg(test)]
mod tests
{
	use crate::parser::source::SourceFile;
	use super::*;

	#[test]
	fn te()
	{
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
Sit
	Lorem
	Ipsum
Dolor
		"#
			.to_string(),
			None,
		));

		let mut reports = vec![];

		//let la = report_label!(source.clone(), 5..9, "Msg".into());
		report_err!(&mut reports, source.clone(), "Some message".into(),
			span(5..9, "Msg".into()),
			span(5..9, "Another".into()),
		);
		println!("Report = {reports:#?}");
	}
}
