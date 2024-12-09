use std::rc::Rc;

use ariadne::Fmt;
use document::document::Document;
use document::document::DocumentAccessors;
use elements::paragraph::Paragraph;
use lsp::definition;
use lsp::semantic::Semantics;
use parser::parser::ParseMode;
use parser::parser::ParserState;
use parser::parser::ReportColors;
use parser::rule::RegexRule;
use parser::source::SourceFile;
use parser::source::Token;
use regex::Captures;
use regex::Regex;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;

fn validate_name(_colors: &ReportColors, name: &str) -> Result<String, String> {
	Ok(name.to_string())
}

fn validate_as(_colors: &ReportColors, as_name: &str) -> Result<String, String> {
	// TODO: Use variable name validation rules
	Ok(as_name.to_string())
}

#[auto_registry::auto_registry(registry = "rules")]
pub struct ImportRule {
	re: [Regex; 1],
}

impl Default for ImportRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r"(?:^|\n)@import(?:\[(.*)\])?[^\S\r\n]+(.*)").unwrap()],
		}
	}
}

impl RegexRule for ImportRule {
	fn name(&self) -> &'static str { "Import" }

	fn previous(&self) -> Option<&'static str> { Some("Paragraph") }

	fn regexes(&self) -> &[Regex] { &self.re }

	fn enabled(&self, mode: &ParseMode, _id: usize) -> bool { !mode.paragraph_only }

	fn on_regex_match<'a>(
		&self,
		_: usize,
		state: &ParserState,
		document: &'a dyn Document<'a>,
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		// Path
		let import_file = match matches.get(2) {
			Some(name) => match validate_name(state.parser.colors(), name.as_str()) {
				Err(msg) => {
					report_err!(
						&mut reports,
						token.source(),
						"Invalid Import Name".into(),
						span(
							name.range(),
							format!(
								"Import name `{}` is invalid. {msg}",
								name.as_str().fg(state.parser.colors().highlight)
							)
						)
					);

					return reports;
				}
				Ok(filename) => {
					let meta = match std::fs::metadata(filename.as_str()) {
						Err(_) => {
							report_err!(
								&mut reports,
								token.source(),
								"Invalid Import Path".into(),
								span(
									name.range(),
									format!(
										"Unable to access file `{}`",
										filename.fg(state.parser.colors().highlight)
									)
								)
							);
							return reports;
						}
						Ok(meta) => meta,
					};

					if !meta.is_file() {
						report_err!(
							&mut reports,
							token.source(),
							"Invalid Import Path".into(),
							span(
								name.range(),
								format!(
									"Path `{}` is not a file!",
									filename.fg(state.parser.colors().highlight)
								)
							)
						);
						return reports;
					}

					filename
				}
			},
			_ => panic!("Invalid name for import"),
		};

		// [Optional] import as
		let import_as = match matches.get(1) {
			Some(as_name) => match validate_as(state.parser.colors(), as_name.as_str()) {
				Ok(as_name) => as_name,
				Err(msg) => {
					report_err!(
						&mut reports,
						token.source(),
						"Invalid Import As".into(),
						span(
							as_name.range(),
							format!(
								"Canot import `{import_file}` as `{}`. {msg}",
								as_name.as_str().fg(state.parser.colors().highlight)
							)
						)
					);

					return reports;
				}
			},
			_ => "".to_string(),
		};

		let import = match SourceFile::new(import_file, Some(token.clone())) {
			Ok(import) => Rc::new(import),
			Err(path) => {
				report_err!(
					&mut reports,
					token.source(),
					"Invalid Import File".into(),
					span(
						token.range.clone(),
						format!("Failed to read content from path `{path}`")
					)
				);
				return reports;
			}
		};

		state.with_state(|new_state| {
			let (import_doc, _) = new_state.parser.parse(
				new_state,
				import.clone(),
				Some(document),
				ParseMode::default(),
			);
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

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			// @import
			let import =
				if token.source().content().as_bytes()[matches.get(0).unwrap().start()] == b'\n' {
					matches.get(0).unwrap().start() + 1
				} else {
					matches.get(0).unwrap().start()
				};
			sems.add(import..import + 7, tokens.import_import);

			if let Some(import_as) = matches.get(1) {
				sems.add(
					import_as.start() - 1..import_as.start(),
					tokens.import_as_sep,
				);
				sems.add(import_as.range(), tokens.import_as);
				sems.add(import_as.end()..import_as.end() + 1, tokens.import_as_sep);
			}

			let path = matches.get(2).unwrap().range();
			sems.add(path, tokens.import_path);
		}

		// Definition point to start of imported document
		definition::from_source(token, &Token::new(0..0, import), &state.shared.lsp);

		reports
	}
}
