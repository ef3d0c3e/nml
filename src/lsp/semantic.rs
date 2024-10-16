use std::any::Any;

use tower_lsp::lsp_types::SemanticToken;
use tower_lsp::lsp_types::SemanticTokenType;

use crate::document::document::Document;
use crate::document::element::Element;
use crate::elements::comment::Comment;
use crate::elements::paragraph::Paragraph;
use crate::elements::section::Section;
use crate::parser::rule::Rule;

use super::parser::LineCursor;

pub trait SemanticProvider: Rule {
	fn get_semantic_tokens(
		&self,
		cursor: &LineCursor,
		match_data: Box<dyn Any>,
	) -> Vec<SemanticToken>;
}

pub mod nml_semantic {
    use tower_lsp::lsp_types::SemanticTokenType;

    pub const SECTION_HEADING: SemanticTokenType = SemanticTokenType::new("type");
    pub const SECTION_NAME: SemanticTokenType = SemanticTokenType::new("string");
    pub const REFERENCE: SemanticTokenType = SemanticTokenType::new("event");
}

pub const LEGEND_TYPE: &[SemanticTokenType] = &[
	SemanticTokenType::COMMENT,
	SemanticTokenType::VARIABLE,
	SemanticTokenType::STRING,
	SemanticTokenType::PARAMETER,
];

// TODO...
pub fn provide(
	semantic_tokens: &mut Vec<SemanticToken>,
	cursor: &mut LineCursor,
	elem: &Box<dyn Element>,
) {
	if cursor.source != elem.location().source() {
		return;
	}

	let prev = cursor.clone();

	/*if let Some(comm) = elem.downcast_ref::<Comment>() {
		cursor.at(elem.location().start());
		let delta_start = if cursor.line == prev.line {
			cursor.line_pos - prev.line_pos
		} else if cursor.line == 0 {
			cursor.line_pos
		} else {
			cursor.line_pos + 1
		};
		semantic_tokens.push(SemanticToken {
			delta_line: (cursor.line - prev.line) as u32,
			delta_start: delta_start as u32,
			length: (elem.location().end() - elem.location().start()) as u32,
			token_type: 0,
			token_modifiers_bitset: 0,
		});
	} else */if let Some(sect) = elem.downcast_ref::<Section>() {
		cursor.at(elem.location().start() + 1);
		eprintln!("section {cursor:#?}");
		let delta_start = if cursor.line == prev.line {
			cursor.line_pos - prev.line_pos
		} else if cursor.line == 0 {
			cursor.line_pos
		} else {
			0
		};
		semantic_tokens.push(SemanticToken {
			delta_line: (cursor.line - prev.line) as u32,
			delta_start: delta_start as u32,
			length: (elem.location().end() - elem.location().start()) as u32,
			token_type: 0,
			token_modifiers_bitset: 0,
		});
	}
}

pub fn semantic_token_from_document(document: &dyn Document) -> Vec<SemanticToken> {
	let mut semantic_tokens = vec![];

	let source = document.source();
	let mut cursor = LineCursor {
		pos: 0,
		line: 0,
		line_pos: 0,
		source: source.clone(),
	};
/*
	semantic_tokens.push(SemanticToken {
		delta_line: 2,
		delta_start: 1,
		length: 5,
		token_type: 0,
		token_modifiers_bitset: 0,
	});

	semantic_tokens.push(SemanticToken {
		delta_line: 1,
		delta_start: 1,
		length: 5,
		token_type: 1,
		token_modifiers_bitset: 0,
	});*/

	document.content().borrow()
		.iter()
		.for_each(|elem| {
			if let Some(container) = elem.as_container()
			{
				container
					.contained()
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
