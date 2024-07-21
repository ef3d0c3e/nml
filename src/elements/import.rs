use mlua::{Function, Lua};
use regex::Regex;
use crate::parser::{parser::{Parser, ReportColors}, rule::RegexRule, source::{Source, SourceFile, Token}};
use ariadne::{Report, Fmt, Label, ReportKind};
use crate::document::document::Document;
use std::{ops::Range, rc::Rc};

use super::paragraph::Paragraph;

pub struct ImportRule {
	re: [Regex; 1],
}

impl ImportRule {
	pub fn new() -> Self {
		Self {
            re: [Regex::new(r"(?:^|\n)@import(?:\[(.*)\])?[^\S\r\n]+(.*)").unwrap()],
        }
	}

    pub fn validate_name(_colors: &ReportColors, name: &str) -> Result<String, String>
    {
        Ok(name.to_string())
    }

    pub fn validate_as(_colors: &ReportColors, as_name: &str) -> Result<String, String>
    {
        // TODO: Use variable name validation rules
        Ok(as_name.to_string())
    }
}

impl RegexRule for ImportRule {
	fn name(&self) -> &'static str { "Import" }

	fn regexes(&self) -> &[Regex] { &self.re }

	fn on_regex_match(&self, _: usize, parser: &dyn Parser, document: &Document, token: Token, matches: regex::Captures) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>
	{
		let mut result = vec![];

        // Path
        let import_file = match matches.get(2)
        {
            Some(name) => {
                match ImportRule::validate_name(parser.colors(), name.as_str())
                {
                    Err(msg) => {
                        result.push(
                            Report::build(ReportKind::Error, token.source(), name.start())
                            .with_message("Invalid name for import")
                            .with_label(
                                Label::new((token.source(), name.range()))
                                .with_message(format!("Import name `{}` is invalid. {msg}",
                                        name.as_str().fg(parser.colors().highlight)))
                                .with_color(parser.colors().error))
                            .finish());

                        return result;
                    },
                    Ok(filename) => {
                        let meta = match std::fs::metadata(filename.as_str())
                        {
                            Err(_) => {
                                result.push(
                                    Report::build(ReportKind::Error, token.source(), name.start())
                                    .with_message("Invalid import path")
                                    .with_label(
                                        Label::new((token.source(), name.range()))
                                        .with_message(format!("Unable to access file `{}`",
                                                filename.fg(parser.colors().highlight)))
                                        .with_color(parser.colors().error))
                                    .finish());
                                    return result;
                            },
                            Ok(meta) => meta
                        };

                        if !meta.is_file()
                        {
                            result.push(
                                Report::build(ReportKind::Error, token.source(), name.start())
                                .with_message("Invalid import path")
                                .with_label(
                                    Label::new((token.source(), name.range()))
                                    .with_message(format!("Path `{}` is not a file!",
                                            filename.fg(parser.colors().highlight)))
                                    .with_color(parser.colors().error))
                                .finish());
                            return result;
                        }

                        filename
                    },
                }
            }
            _ => panic!("Invalid name for import")
        };

        // [Optional] import as
        let import_as = match matches.get(1)
        {
            Some(as_name) => {
                match ImportRule::validate_as(parser.colors(), as_name.as_str())
                {
                    Ok(as_name) => as_name,
                    Err(msg) => {
                        result.push(
                            Report::build(ReportKind::Error, token.source(), as_name.start())
                            .with_message("Invalid name for import as")
                            .with_label(
                                Label::new((token.source(), as_name.range()))
                                .with_message(format!("Canot import `{import_file}` as `{}`. {msg}",
                                        as_name.as_str().fg(parser.colors().highlight)))
                                .with_color(parser.colors().error))
                            .finish());

                        return result;
                    },
                }
            }
            _ => "".to_string()
        };

        let import = match SourceFile::new(import_file, Some(token.clone()))
		{
			Ok(import) => Rc::new(import),
			Err(path) => {
                result.push(
					Report::build(ReportKind::Error, token.source(), token.start())
					.with_message("Unable to read file content")
					.with_label(
						Label::new((token.source(), token.range))
						.with_message(format!("Failed to read content from path `{path}`"))
						.with_color(parser.colors().error))
					.finish());
				return result;
			}
		};

		// TODO
        let import_doc = parser.parse(import, Some(&document));

        document.merge(import_doc, Some(&import_as));

		// Close paragraph
		if document.last_element::<Paragraph>(false).is_some()
		{
			parser.push(document, Box::new(Paragraph::new(
				Token::new(token.end()..token.end(), token.source())
			)));
		}

        return result;
	}

	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Vec<(String, Function<'lua>)> { vec![] }
}
