use std::any::Any;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use tower_lsp::lsp_types::SemanticToken;
use tower_lsp::lsp_types::SemanticTokenModifier;
use tower_lsp::lsp_types::SemanticTokenType;

use crate::parser::source::LineCursor;
use crate::parser::source::Source;


pub const TOKEN_TYPE: &[SemanticTokenType] = &[
    SemanticTokenType::NAMESPACE,
    SemanticTokenType::TYPE,
    SemanticTokenType::CLASS,
    SemanticTokenType::ENUM,
    SemanticTokenType::INTERFACE,
    SemanticTokenType::STRUCT,
    SemanticTokenType::TYPE_PARAMETER,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::ENUM_MEMBER,
    SemanticTokenType::EVENT,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::METHOD,
    SemanticTokenType::MACRO,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::MODIFIER,
    SemanticTokenType::COMMENT,
    SemanticTokenType::STRING,
    SemanticTokenType::NUMBER,
    SemanticTokenType::REGEXP,
    SemanticTokenType::OPERATOR,
    SemanticTokenType::DECORATOR,
];

pub const TOKEN_MODIFIERS: &[SemanticTokenModifier] = &[
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFINITION,
    SemanticTokenModifier::READONLY,
    SemanticTokenModifier::STATIC,
    SemanticTokenModifier::DEPRECATED,
    SemanticTokenModifier::ABSTRACT,
    SemanticTokenModifier::ASYNC,
    SemanticTokenModifier::MODIFICATION,
    SemanticTokenModifier::DOCUMENTATION,
    SemanticTokenModifier::DEFAULT_LIBRARY,
];

fn token_index(name: &str) -> u32
{
	TOKEN_TYPE.iter()
		.enumerate()
		.find(|(_, token)| token.as_str() == name)
		.map(|(index, _)| index as u32)
		.unwrap_or(0)
}
fn modifier_index(name: &str) -> u32
{
	TOKEN_MODIFIERS.iter()
		.enumerate()
		.find(|(_, token)| token.as_str() == name)
		.map(|(index, _)| index as u32)
		.unwrap_or(0)
}
macro_rules! token {
	($key:expr) => {
		{
			(token_index($key), 0)
		}
	};
	($key:expr, $($mods:tt)*) => {
		{
			let mut bitset : u32 = 0;
			$(
				bitset |= 1 << modifier_index($mods);
			)*
				(token_index($key), bitset)
		}
	};
}

#[derive(Debug)]
pub struct Tokens
{
	pub section_heading: (u32, u32),
	pub section_reference: (u32, u32),
	pub section_kind: (u32, u32),
	pub section_name: (u32, u32),
}

impl Tokens
{
	pub fn new() -> Self
	{
		Self {
			section_heading : token!("number"),
			section_reference : token!("enum", "async"),
			section_kind : token!("enum"),
			section_name : token!("string"),
		}
	}
}

/// Semantics for a buffer
#[derive(Debug)]
pub struct Semantics {
	/// The tokens
	pub token: Tokens,

	/// The current cursor
	cursor: RefCell<LineCursor>,

	/// Semantic tokens
	pub tokens: RefCell<Vec<SemanticToken>>,
}

impl Semantics {
	pub fn new(source: Rc<dyn Source>) -> Semantics {
		Self {
			token: Tokens::new(),
			cursor: RefCell::new(LineCursor::new(source)),
			tokens: RefCell::new(vec![]),
		}
	}

	pub fn add(
		&self,
		source: Rc<dyn Source>,
		range: Range<usize>,
		token: (u32, u32)
	) {
		let mut tokens = self.tokens.borrow_mut();
		let mut cursor = self.cursor.borrow_mut();
		let mut current = cursor.clone();
		cursor.move_to(range.start);

		while cursor.pos != range.end {
			let end = source.content()[cursor.pos..]
				.find('\n')
				.unwrap_or(source.content().len() - cursor.pos);
			let len = usize::min(range.end - cursor.pos, end);

			let delta_line = cursor.line - current.line;
			let delta_start = if delta_line == 0 {
				if let Some(last) = tokens.last() {
					cursor.line_pos - current.line_pos + last.length as usize
				} else {
					cursor.line_pos - current.line_pos
				}
			} else {
				cursor.line_pos
			};

			//eprintln!("CURRENT={:#?}, CURS={:#?}", current, cursor);
			tokens.push(SemanticToken {
				delta_line: delta_line as u32,
				delta_start: delta_start as u32,
				length: len as u32,
				token_type: token.0,
				token_modifiers_bitset: token.1
			});
			current = cursor.clone();
			let pos = cursor.pos;
			cursor.move_to(pos + len);
		}
	}
}
