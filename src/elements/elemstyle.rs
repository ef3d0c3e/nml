use std::ops::Range;
use std::rc::Rc;

use ariadne::{Fmt, Label, Report, ReportKind};
use regex::{Captures, Regex};

use crate::document::document::Document;
use crate::document::{self};
use crate::parser::parser::Parser;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;

use super::variable::VariableRule;

pub struct ElemStyleRule {
	re: [Regex; 1],
}

impl ElemStyleRule {
	pub fn new() -> Self {
		Self {
			re: [Regex::new(r"(?:^|\n)@@(.*?)=((?:\\\n|.)*)").unwrap()],
		}
	}
}

impl RegexRule for ElemStyleRule {
	fn name(&self) -> &'static str { "Element Style" }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn on_regex_match<'a>(
		&self,
		_: usize,
		parser: &dyn Parser,
		_document: &'a dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let style = if let Some(key) = matches.get(1)
		{
			let trimmed = key.as_str().trim_start().trim_end();
			
			// Check if empty
			if trimmed.is_empty()
			{
				reports.push(
				Report::build(ReportKind::Error, token.source(), key.start())
					.with_message("Empty Style Key")
					.with_label(
						Label::new((token.source(), key.range()))
						.with_message(format!(
								"Expected a non-empty style key",
						))
						.with_color(parser.colors().error),
					)
					.finish());
				return reports;
			}

			// Check if key exists
			if !parser.is_registered(trimmed)
			{
				reports.push(
				Report::build(ReportKind::Error, token.source(), key.start())
					.with_message("Unknown Style Key")
					.with_label(
						Label::new((token.source(), key.range()))
						.with_message(format!(
								"Could not find a style with key: {}",
								trimmed.fg(parser.colors().info)
						))
						.with_color(parser.colors().error),
					)
					.finish());

				return reports;
			}
			
			parser.current_style(trimmed)
		} else { panic!("Unknown error") };

		// Get value
		let new_style = if let Some(value) = matches.get(2) {
			let value_str = match VariableRule::validate_value(value.as_str()) {
				Err(err) => {
					reports.push(
						Report::build(ReportKind::Error, token.source(), value.start())
							.with_message("Invalid Style Value")
							.with_label(
								Label::new((token.source(), value.range()))
								.with_message(format!(
										"Value `{}` is not allowed: {err}",
										value.as_str().fg(parser.colors().highlight)
								))
								.with_color(parser.colors().error),
							)
							.finish());
					return reports;
				}
				Ok(value) => value,
			};

			// Attempt to serialize
			match style.from_json(value_str.as_str())
			{
				Err(err) => {
					reports.push(
						Report::build(ReportKind::Error, token.source(), value.start())
						.with_message("Invalid Style Value")
						.with_label(
							Label::new((token.source(), value.range()))
							.with_message(format!(
									"Failed to serialize `{}` into style with key `{}`: {err}",
									value_str.fg(parser.colors().highlight),
									style.key().fg(parser.colors().info)
							))
							.with_color(parser.colors().error),
						)
						.finish());
						return reports;
				},
				Ok(style) => style,
			}
		} else { panic!("Unknown error") };

		parser.set_style(new_style);

		reports
	}
}
