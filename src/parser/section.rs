use regex::Regex;
use super::rule::{RuleResult, RuleError, SyntaxRule};
use super::super::files::cursor::Cursor;
use super::super::files::token::Token;
use super::super::syntax::element::{Element, ReferenceableElement};

pub mod SectionKind 
{
    pub const NONE : u8 = 0x00;
    pub const NO_TOC : u8 = 0x01;
    pub const NO_NUMBER : u8 = 0x02;
}

pub struct Section
{
    title: String,
    reference: Option<String>,
    section_kind: u8,
    depth: usize,
}

impl Section
{
    pub fn new<'h>(_title: String, _reference: Option<String>, kind: u8, _depth: usize) -> Section
    {
        Section {
            title: _title,
            reference: _reference,
            section_kind: kind,
            depth: _depth,
        }
    }
}

impl Element for Section
{
    fn element_name(&self) -> &'static str { "Section" }
}

impl ReferenceableElement for Section
{
    fn reference_name(&self) -> Option<&String> { self.reference.as_ref() }
}


// TODO: Single file for grammar + element, and add `Rule` suffix for rules
pub struct SectionRule
{
	regex: Regex,
}

impl SectionRule
{
    pub fn new() -> SectionRule
    {
		SectionRule
		{
			regex: regex::Regex::new(r"(?:^|\n)(#{1,})(\{.*\})?((?:\*|\+){0,})?((?:\t| ){0,})(.*)").unwrap()
		}
    }
}

impl SyntaxRule for SectionRule
{
	fn name(&self) -> &'static str { "Section" }

    fn next_match<'a>(&self, cursor: &'a Cursor) -> Option<usize>
    {
		match self.regex.find_at(&cursor.content, cursor.position)
		{
			Some(m) => Some(m.start()),
			None => None
		}
    }

	fn on_match<'a>(&self, cursor: &'a Cursor) -> Result<(Token<'a>, RuleResult), RuleError<'a>>
	{
        let m = self.regex.captures_at(&cursor.content, cursor.position).unwrap(); // Capture match
		
		let section_depth = match m.get(1)
        {
            Some(depth) => {
				if depth.len() > 6
				{
					return Err(RuleError::new(&cursor, m.get(1),
					format!("Section depth must not be greater than 6, got `{}` (depth: {})", depth.as_str(), depth.len())))
				}

				depth.len()
            }
            _ => return Err(RuleError::new(&cursor, m.get(1), String::from("Empty section depth")))
        };

		// Spacing
		match m.get(4)
		{
			Some(spacing) => {
				if spacing.as_str().is_empty() || !spacing.as_str().chars().all(|c| c == ' ' || c == '\t')
				{
					return Err(RuleError::new(&cursor, m.get(4),
					format!("Sections require spacing made of spaces or tab before the section's title, got: `{}`", spacing.as_str())))
				}
			}
			_ => return Err(RuleError::new(&cursor, m.get(4),
			String::from("Sections require spacing made of spaces or tab before the section's title")))
		}

		let section_refname = match m.get(2)
		{
			Some(reference) => {
				// TODO: Validate reference name
				// TODO: After parsing, check for duplicate references
				Some(String::from(reference.as_str()))
			}
			_ => None
		};

		let section_kind = match m.get(3)
		{
			Some(kind) => {
				match kind.as_str() {
					"*+" => SectionKind::NO_NUMBER | SectionKind::NO_TOC,
					"*" => SectionKind::NO_NUMBER,
					"+" => SectionKind::NO_TOC,
					"" => SectionKind::NONE,
					_ => return Err(RuleError::new(&cursor, m.get(3),
					format!("Section kind must be either `*` for unnumbered, `+` to hide from TOC or `*+`, got `{}`. Leave empty for normal sections", kind.as_str())))
				}
			}
			_ => SectionKind::NONE,
		};

		let section_title = match m.get(5) {
			Some(title) => match title.as_str() {
				"" => return Err(RuleError::new(&cursor, m.get(5),
				String::from("Sections require a non-empty title"))),
				_ => String::from(title.as_str())
			}
			_ => return Err(RuleError::new(&cursor, m.get(5),
			String::from("Sections require a non-empty title")))
		};

        let section = Box::new(Section::new(
            section_title,
            section_refname,
            section_kind,
            section_depth));

        Ok((Token::from(cursor, m.get(0).unwrap()), RuleResult::new(m.get(0).unwrap().len(), section)))
	}
}
