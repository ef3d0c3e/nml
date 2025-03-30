
use std::env::current_dir;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use ariadne::Fmt;
use graphviz_rust::print;
use parser::rule::RegexRule;
use parser::source::Token;
use regex::{Captures, Regex};

use crate::parser::reports::macros::*;
use crate::parser::source::SourceFile;
use crate::parser::{reports::*, util};
use crate::parser::state::ParseMode;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::{TranslationAccessors, TranslationUnit};

use super::elem::Import;

#[auto_registry::auto_registry(registry = "rules")]
pub struct ImportRule {
	re: [Regex; 1],
}

impl Default for ImportRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r#"(?:^|\n)@import\s+(")?((?:[^"\\]|\\.)*)(")?([^\n]*)"#).unwrap()],
		}
	}
}

impl RegexRule for ImportRule {
	fn name(&self) -> &'static str { "Import" }

	fn previous(&self) -> Option<&'static str> { Some("Break") }

	fn regexes(&self) -> &[Regex] { &self.re }

	fn enabled(&self, mode: &ParseMode, _id: usize) -> bool { !mode.paragraph_only }

	fn on_regex_match<'u>(
		&self,
		_index: usize,
		unit: &mut TranslationUnit<'u>,
		token: Token,
		captures: Captures,
	) {
		let path = captures.get(2).unwrap();

		// Missing starting '"'
		if captures.get(1).is_none()
		{
			report_err!(
				unit,
				token.source(),
				"Invalid import".into(),
				span(
					path.start()..path.start(),
					format!("Missing `{}` delimiter for import", "\"".fg(unit.colors().info))
				),
			);
			return
		}

		// Missing ending '"'
		if captures.get(3).is_none()
		{
			report_err!(
				unit,
				token.source(),
				"Invalid import".into(),
				span(
					path.end()-1..path.end()-1,
					format!("Missing `{}` delimiter for import", "\"".fg(unit.colors().info))
				),
			);
			return
		}

		// Leftover
		if !captures.get(4).unwrap().as_str().trim_start().is_empty()
		{
			report_err!(
				unit,
				token.source(),
				"Invalid import".into(),
				span(
					captures.get(4).unwrap().range(),
					format!("Unexpected content here")
				),
			);
			return
		}

		// Build relative path
		let path_content = util::escape_text('\\', "\"", path.as_str(), false);
		let mut rel_path = std::path::PathBuf::from(unit.token().source().name());
		rel_path.pop();
		rel_path.push(path_content.as_str());
		let path_buf = match std::fs::canonicalize(&rel_path) {
			Ok(path) => path,
			Err(err) => {
				report_err!(
					unit,
					token.source(),
					"Invalid import".into(),
					span(
						path.range(),
						format!("Failed to canonicalize `{:#?}`: {err}", path_content.fg(unit.colors().highlight))
					),
					note(format!("Current working directory: {}", current_dir().unwrap().to_string_lossy().fg(unit.colors().info) ))
				);
				return
			},
		};

		// Parse imported
		let source = match SourceFile::new(path_buf.to_str().expect("Invalid path").to_string(), Some(token.clone())) {
			Ok(source) => source,
			Err(err) => {
				report_err!(
					unit,
					token.source(),
					"Invalid import".into(),
					span(
						path.range(),
						format!("{err}")
					)
				);
				return;
			},
		};

		let content = unit.with_child(Arc::new(source), ParseMode::default(), true, |unit, scope| {
			unit.parser.parse(unit);
			scope
		});

		unit.get_scope()
			.add_import(content.clone());
		unit.add_content(Rc::new(Import {
			location: token,
			content: vec![content],
		}));
	}

}
