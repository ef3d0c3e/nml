use mlua::{Function, Lua};
use regex::{Captures, Regex};
use crate::{document::document::Document, parser::{parser::Parser, rule::RegexRule, source::{Source, Token}}};
use ariadne::{Report, Label, ReportKind};
use crate::{compiler::compiler::Compiler, document::element::{ElemKind, Element}};
use std::{ops::Range, rc::Rc};

#[derive(Debug)]
pub struct Comment {
    location: Token,
	content: String,
}

impl Comment
{
    pub fn new(location: Token, content: String ) -> Self {
        Self { location: location, content }
    }
}

impl Element for Comment
{
    fn location(&self) -> &Token { &self.location }
    fn kind(&self) -> ElemKind { ElemKind::Invisible }
    fn element_name(&self) -> &'static str { "Comment" }
    fn to_string(&self) -> String { format!("{self:#?}") }
    fn compile(&self, _compiler: &Compiler, _document: &dyn Document)
		-> Result<String, String> {
		Ok("".to_string())
    }
}

pub struct CommentRule {
	re: [Regex; 1],
}

impl CommentRule {
	pub fn new() -> Self {
		Self { re: [Regex::new(r"\s*::(.*)").unwrap()] }
	}
}

impl RegexRule for CommentRule {
	fn name(&self) -> &'static str { "Comment" }

	fn regexes(&self) -> &[Regex] { &self.re }

    fn on_regex_match<'a>(&self, _: usize, parser: &dyn Parser, document: &'a dyn Document, token: Token, matches: Captures)
		-> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let content = match matches.get(1)
		{
			None => panic!("Unknown error"),
			Some(comment) => {
				let trimmed = comment.as_str().trim_start().trim_end().to_string();
				if trimmed.is_empty()
				{
					reports.push(
						Report::build(ReportKind::Warning, token.source(), comment.start())
						.with_message("Empty comment")
						.with_label(
							Label::new((token.source(), comment.range()))
							.with_message("Comment is empty")
							.with_color(parser.colors().warning))
						.finish());
				}

				trimmed
			}
		};
		
        parser.push(document, Box::new(
            Comment::new(
				token.clone(),
	            content
            )
        ));

        return reports;
	}

	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Vec<(String, Function<'lua>)> { vec![] }
}
