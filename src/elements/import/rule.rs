
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
use crate::parser::rule::RuleTarget;
use crate::parser::source::SourceFile;
use crate::parser::{reports::*, util};
use crate::parser::state::ParseMode;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::{TranslationAccessors, TranslationUnit};

use super::completion::ImportCompletion;
use super::elem::Import;

#[auto_registry::auto_registry(registry = "rules")]
pub struct ImportRule {
	re: [Regex; 1],
}

impl Default for ImportRule {
	fn default() -> Self {
		Self {
			re: [Regex::new(r#"(?:^|\n)(@import)\s+(")?((?:[^"\\]|\\.)*)(")?([^\n]*)"#).unwrap()],
		}
	}
}

impl RegexRule for ImportRule {
	fn name(&self) -> &'static str { "Import" }

	fn target(&self) -> RuleTarget {
	    RuleTarget::Command
	}

	fn regexes(&self) -> &[Regex] { &self.re }

	fn enabled(&self, mode: &ParseMode, _id: usize) -> bool { !mode.paragraph_only }

	fn on_regex_match<'u>(
		&self,
		_index: usize,
		unit: &mut TranslationUnit<'u>,
		token: Token,
		captures: Captures,
	) {
		let path = captures.get(3).unwrap();

		// Missing starting '"'
		if captures.get(2).is_none()
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
		if captures.get(4).is_none()
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
		if !captures.get(5).unwrap().as_str().trim_start().is_empty()
		{
			report_err!(
				unit,
				token.source(),
				"Invalid import".into(),
				span(
					captures.get(5).unwrap().range(),
					format!("Unexpected content here")
				),
			);
			return
		}

		unit.with_lsp(|lsp| lsp.with_semantics(token.source(), |sems, tokens|{
			sems.add(captures.get(1).unwrap().range(), tokens.import);
			sems.add(captures.get(2).unwrap().start()..captures.get(5).unwrap().end(), tokens.import_path);
		}));

		// Build path
		let path_content = util::escape_text('\\', "\"", path.as_str(), false);
		let path_buf = match std::fs::canonicalize(&path_content) {
			Ok(path) => path,
			Err(err) => {
				report_err!(
					unit,
					token.source(),
					"Invalid import".into(),
					span(
						path.range(),
						format!("Failed to canonicalize `{}`: {err}", path_content.fg(unit.colors().highlight))
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
			unit.with_lsp(|lsp| lsp.add_definition(token.clone(), &Token::new(0..0, scope.borrow().source())));
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

	fn completion(&self) -> Option<Box<dyn lsp::completion::CompletionProvider + 'static + Send + Sync>> {
		Some(Box::new(ImportCompletion {}))
	}
}
