use crate::document::document::Document;
use crate::document::document::DocumentAccessors;
use crate::parser::parser::ParserState;
use crate::parser::parser::ReportColors;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::SourceFile;
use crate::parser::source::Token;
use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use regex::Captures;
use regex::Regex;
use std::ops::Range;
use std::rc::Rc;

use super::paragraph::Paragraph;

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::import")]
pub struct ImportRule {
	re: [Regex; 1],
}

impl ImportRule {
	pub fn new() -> Self {
		Self {
			re: [Regex::new(r"(?:^|\n)@import(?:\[(.*)\])?[^\S\r\n]+(.*)").unwrap()],
		}
	}

	pub fn validate_name(_colors: &ReportColors, name: &str) -> Result<String, String> {
		Ok(name.to_string())
	}

	pub fn validate_as(_colors: &ReportColors, as_name: &str) -> Result<String, String> {
		// TODO: Use variable name validation rules
		Ok(as_name.to_string())
	}
}

impl RegexRule for ImportRule {
	fn name(&self) -> &'static str { "Import" }
	fn previous(&self) -> Option<&'static str> { Some("Paragraph") }

	fn regexes(&self) -> &[Regex] { &self.re }

	fn on_regex_match<'a>(
		&self,
		_: usize,
		state: &ParserState,
		document: &'a dyn Document<'a>,
		token: Token,
		matches: Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut result = vec![];

		// Path
		let import_file = match matches.get(2) {
			Some(name) => match ImportRule::validate_name(state.parser.colors(), name.as_str()) {
				Err(msg) => {
					result.push(
						Report::build(ReportKind::Error, token.source(), name.start())
							.with_message("Invalid name for import")
							.with_label(
								Label::new((token.source(), name.range()))
									.with_message(format!(
										"Import name `{}` is invalid. {msg}",
										name.as_str().fg(state.parser.colors().highlight)
									))
									.with_color(state.parser.colors().error),
							)
							.finish(),
					);

					return result;
				}
				Ok(filename) => {
					let meta = match std::fs::metadata(filename.as_str()) {
						Err(_) => {
							result.push(
								Report::build(ReportKind::Error, token.source(), name.start())
									.with_message("Invalid import path")
									.with_label(
										Label::new((token.source(), name.range()))
											.with_message(format!(
												"Unable to access file `{}`",
												filename.fg(state.parser.colors().highlight)
											))
											.with_color(state.parser.colors().error),
									)
									.finish(),
							);
							return result;
						}
						Ok(meta) => meta,
					};

					if !meta.is_file() {
						result.push(
							Report::build(ReportKind::Error, token.source(), name.start())
								.with_message("Invalid import path")
								.with_label(
									Label::new((token.source(), name.range()))
										.with_message(format!(
											"Path `{}` is not a file!",
											filename.fg(state.parser.colors().highlight)
										))
										.with_color(state.parser.colors().error),
								)
								.finish(),
						);
						return result;
					}

					filename
				}
			},
			_ => panic!("Invalid name for import"),
		};

		// [Optional] import as
		let import_as = match matches.get(1) {
			Some(as_name) => match ImportRule::validate_as(state.parser.colors(), as_name.as_str())
			{
				Ok(as_name) => as_name,
				Err(msg) => {
					result.push(
						Report::build(ReportKind::Error, token.source(), as_name.start())
							.with_message("Invalid name for import as")
							.with_label(
								Label::new((token.source(), as_name.range()))
									.with_message(format!(
										"Canot import `{import_file}` as `{}`. {msg}",
										as_name.as_str().fg(state.parser.colors().highlight)
									))
									.with_color(state.parser.colors().error),
							)
							.finish(),
					);

					return result;
				}
			},
			_ => "".to_string(),
		};

		let import = match SourceFile::new(import_file, Some(token.clone())) {
			Ok(import) => Rc::new(import),
			Err(path) => {
				result.push(
					Report::build(ReportKind::Error, token.source(), token.start())
						.with_message("Unable to read file content")
						.with_label(
							Label::new((token.source(), token.range))
								.with_message(format!("Failed to read content from path `{path}`"))
								.with_color(state.parser.colors().error),
						)
						.finish(),
				);
				return result;
			}
		};

		state.with_state(|new_state| {
			let (import_doc, _) = new_state.parser.parse(new_state, import, Some(document));
			document.merge(import_doc.content(), import_doc.scope(), Some(&import_as));
		});

		// Close paragraph
		// TODO2: Check if this is safe to remove
		if document.last_element::<Paragraph>().is_none() {
			state.push(
				document,
				Box::new(Paragraph {
					location: Token::new(token.end()..token.end(), token.source()),
					content: Vec::new(),
				}),
			);
		}

		result
	}
}
