use regex::Captures;
use super::super::syntax::element::Element;
use super::super::files::cursor::Cursor;
use super::super::files::token::Token;

pub struct RuleResult
{
    length: usize,
    pub elements: Vec<Box<dyn Element>>,
}

impl RuleResult
{
    pub fn new(_length: usize, elem: Box<dyn Element>) -> RuleResult
    {
        RuleResult
        {
            length: _length,
            elements: vec![elem],
        }
    }
}

#[derive(Debug)]
pub struct RuleError<'a>
{
    // where: token
	cursor: &'a Cursor<'a>,
    mat: Option<regex::Match<'a>>,
    message: String,
}

impl<'a> RuleError<'a>
{
    pub fn new(_cursor: &'a Cursor<'a>, _match: Option<regex::Match<'a>>, _message: String) -> RuleError<'a>
    {
		RuleError
		{
			cursor: _cursor,
			mat: _match,
			message: _message,
		}
    }
}

pub trait SyntaxRule
{
	fn name(&self) -> &'static str;
    fn next_match<'a>(&self, cursor: &'a Cursor) -> Option<usize>;
	fn on_match<'a>(&self, cursor: &'a Cursor) -> Result<(Token<'a>, RuleResult), RuleError<'a>>;
}
