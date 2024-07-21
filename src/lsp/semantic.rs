use std::any::Any;

use tower_lsp::lsp_types::{SemanticToken, SemanticTokenType};

use crate::{document::{document::Document, element::Element}, elements::{comment::Comment, paragraph::Paragraph, section::Section}, parser::rule::Rule};

use super::parser::LineCursor;

pub trait SemanticProvider: Rule
{
	fn get_semantic_tokens(&self, cursor: &LineCursor, match_data: Box<dyn Any>) -> Vec<SemanticToken>;
}

pub const LEGEND_TYPE : &[SemanticTokenType] = &[
    SemanticTokenType::COMMENT,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::STRING,
    SemanticTokenType::PARAMETER,
];

// TODO...
pub fn provide(semantic_tokens: &mut Vec<SemanticToken>, cursor: &mut LineCursor, elem: &Box<dyn Element>) {
	if cursor.source != elem.location().source() { return }

	let prev = cursor.clone();

	if let Some(comm) = elem.downcast_ref::<Comment>()
	{
		cursor.at(elem.location().start());
		let delta_start = if cursor.line == prev.line
		{
			cursor.line_pos - prev.line_pos
		} else if cursor.line == 0 { cursor.line_pos }
		else { cursor.line_pos+1 };
		semantic_tokens.push(SemanticToken {
			delta_line: (cursor.line-prev.line) as u32,
			delta_start: delta_start as u32,
			length: (elem.location().end() - elem.location().start()) as u32,
			token_type: 0,
			token_modifiers_bitset: 0,
		});
	}
	else if let Some(sect) = elem.downcast_ref::<Section>()
	{
		eprintln!("section");
		cursor.at(elem.location().start());
		let delta_start = if cursor.line == prev.line
		{
			cursor.line_pos - prev.line_pos
		} else if cursor.line == 0 { cursor.line_pos }
		else { cursor.line_pos+1 };
		semantic_tokens.push(SemanticToken {
			delta_line: (cursor.line-prev.line) as u32,
			delta_start: delta_start as u32,
			length: (elem.location().end() - elem.location().start()) as u32,
			token_type: 0,
			token_modifiers_bitset: 0,
		});
	}
}

pub fn semantic_token_from_document(document: &Document) -> Vec<SemanticToken>
{
	let mut semantic_tokens = vec![];

	let source = document.source();
	let mut cursor = LineCursor {
		pos: 0,
		line: 0,
		line_pos: 0,
		source: source.clone()
	};

	document.content.borrow()
		.iter()
		.for_each(|elem| {
			if let Some(paragraph) = elem.downcast_ref::<Paragraph>()
			{
				paragraph.content
					.iter()
					.for_each(|elem| provide(&mut semantic_tokens, &mut cursor, elem));
			}
			else
			{
				provide(&mut semantic_tokens, &mut cursor, elem);
			}
		});

	semantic_tokens
}
