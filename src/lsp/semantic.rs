use std::collections::VecDeque;
use std::ops::Range;
use std::sync::Arc;

use parking_lot::RwLock;
use tower_lsp::lsp_types::SemanticToken;
use tower_lsp::lsp_types::SemanticTokenModifier;
use tower_lsp::lsp_types::SemanticTokenType;

use crate::parser::source::LineCursor;
use crate::parser::source::OffsetEncoding;
use crate::parser::source::Source;
use crate::parser::source::SourceFile;
use crate::parser::source::SourcePosition;
use crate::parser::source::VirtualSource;

use super::data::LangServerData;

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

fn token_index(name: &str) -> u32 {
	TOKEN_TYPE
		.iter()
		.enumerate()
		.find(|(_, token)| token.as_str() == name)
		.map(|(index, _)| index as u32)
		.unwrap_or(0)
}
fn modifier_index(name: &str) -> u32 {
	TOKEN_MODIFIERS
		.iter()
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
	($key:expr, $($mods:tt),*) => {
		{
			let mut bitset : u32 = 0;
			$(
				bitset |= 1 << modifier_index($mods);
			)*
			(token_index($key), bitset)
		}
	};
}

/// Predefined list of tokens
#[derive(Debug)]
pub struct Tokens {
	pub command: (u32, u32),

	pub import: (u32, u32),
	pub import_path: (u32, u32),

	pub anchor_refname: (u32, u32),

	pub prop_equal: (u32, u32),
	pub prop_comma: (u32, u32),
	pub prop_name: (u32, u32),
	pub prop_value: (u32, u32),

	pub variable_name: (u32, u32),
	pub variable_sep: (u32, u32),
	pub variable_val_int: (u32, u32),
	pub variable_val_string: (u32, u32),
	pub variable_val_block: (u32, u32),

	pub link_display_sep: (u32, u32),
	pub link_url_sep: (u32, u32),
	pub link_url: (u32, u32),

	pub internal_link_ref_sep: (u32, u32),
	pub internal_link_display_sep: (u32, u32),
	pub internal_link_ref: (u32, u32),

	pub style_marker: (u32, u32),

	pub lua_delimiter: (u32, u32),
	pub lua_prop_sep: (u32, u32),
	pub lua_kind: (u32, u32),
	pub lua_content: (u32, u32),

	pub tex_sep: (u32, u32),
	pub tex_content: (u32, u32),
	pub tex_prop_sep: (u32, u32),

	pub graphviz_sep: (u32, u32),
	pub graphviz_content: (u32, u32),
	pub graphviz_prop_sep: (u32, u32),

	pub list_bullet: (u32, u32),
	pub list_prop_sep: (u32, u32),
	pub list_bullet_type: (u32, u32),

	pub heading_depth: (u32, u32),
	pub heading_refname_sep: (u32, u32),
	pub heading_refname: (u32, u32),
	pub heading_kind: (u32, u32),

	pub code_sep: (u32, u32),
	pub code_prop_sep: (u32, u32),
	pub code_lang: (u32, u32),
	pub code_title: (u32, u32),
	pub code_content: (u32, u32),

	pub raw_sep: (u32, u32),
	pub raw_kind: (u32, u32),
	pub raw_content: (u32, u32),

	pub comment: (u32, u32),
}

impl Default for Tokens {
	fn default() -> Self {
		Self {
			command: token!("function"),

			import: token!("function"),
			import_path: token!("string"),

			anchor_refname: token!("parameter"),

			prop_equal: token!("operator"),
			prop_comma: token!("operator"),
			prop_name: token!("class"),
			prop_value: token!("enum"),

			variable_name: token!("constructor", "async"),
			variable_sep: token!("operator"),
			variable_val_int: token!("number"),
			variable_val_string: token!("string"),
			variable_val_block: token!("operator"),

			link_display_sep: token!("macro"),
			link_url_sep: token!("macro"),
			link_url: token!("function", "readonly", "abstract", "abstract"),

			internal_link_ref_sep: token!("macro"),
			internal_link_display_sep: token!("macro"),
			internal_link_ref: token!("parameter"),

			style_marker: token!("decorator"),

			lua_delimiter: token!("function"),
			lua_prop_sep: token!("macro"),
			lua_kind: token!("property"),
			lua_content: token!("regexp"),

			tex_sep: token!("property"),
			tex_content: token!("regexp"),
			tex_prop_sep: token!("macro"),

			graphviz_sep: token!("function"),
			graphviz_content: token!("regexp"),
			graphviz_prop_sep: token!("macro"),

			list_bullet: token!("macro"),
			list_prop_sep: token!("macro"),
			list_bullet_type: token!("float"),

			heading_depth: token!("macro"),
			heading_refname_sep: token!("macro"),
			heading_refname: token!("parameter"),
			heading_kind: token!("operator"),

			code_sep: token!("macro"),
			code_prop_sep: token!("macro"),
			code_lang: token!("function"),
			code_title: token!("property"),
			code_content: token!("regexp"),

			raw_sep: token!("macro"),
			raw_kind: token!("function"),
			raw_content: token!("regexp"),

			comment: token!("comment"),
		}
	}
}

/// Per file semantic tokens
#[derive(Debug)]
pub struct SemanticsData {
	/// The current cursor
	cursor: RwLock<LineCursor>,

	/// Semantic tokens that can't be added directly
	pub semantic_queue: RwLock<VecDeque<(Range<usize>, (u32, u32))>>,

	/// Semantic tokens
	pub tokens: RwLock<Vec<SemanticToken>>,
}

impl SemanticsData {
	pub fn new(source: Arc<dyn Source>) -> Self {
		Self {
			cursor: RwLock::new(LineCursor::new(source, OffsetEncoding::Utf16)),
			semantic_queue: RwLock::new(VecDeque::new()),
			tokens: RwLock::new(vec![]),
		}
	}
}

/// Temporary data returned by [`Self::from_source_impl`]
#[derive(Debug)]
pub struct Semantics<'lsp> {
	pub(self) sems: &'lsp SemanticsData,
	// The source used when resolving the parent source
	pub(self) original_source: Arc<dyn Source>,
	/// The resolved parent source
	pub(self) source: Arc<dyn Source>,
}

impl<'lsp> Semantics<'lsp> {
	fn from_source_impl(
		source: Arc<dyn Source>,
		lsp: &'lsp LangServerData,
		original_source: Arc<dyn Source>,
	) -> Option<Self> {
		if (source.name().starts_with(":LUA:") || source.name().starts_with(":VAR:"))
			&& source.downcast_ref::<VirtualSource>().is_some()
		{
			return None;
		}

		if let Some(location) = source
			.clone()
			.downcast_ref::<VirtualSource>()
			.map(|parent| parent.location())
			.unwrap_or(None)
		{
			return Self::from_source_impl(location.source(), lsp, original_source);
		} else if source.downcast_ref::<SourceFile>().is_some() {
			return lsp.semantic_data.get(&(source.clone())).map(|sems| Self {
				sems,
				source,
				original_source,
			});
		}
		None
	}

	pub fn from_source(
		source: Arc<dyn Source>,
		lsp: &'lsp LangServerData,
	) -> Option<Semantics<'lsp>> {
		Self::from_source_impl(source.clone(), lsp, source)
	}

	/// Method that should be called at the end of parsing
	///
	/// This function will process the end of the semantic queue
	pub fn on_document_end(lsp: &'lsp LangServerData, source: Arc<dyn Source>) {
		if source.content().is_empty() {
			return;
		}
		let pos = source.original_position(source.content().len() - 1).1;
		if let Some(sems) = Self::from_source(source, lsp) {
			sems.process_queue(pos);
		}
	}

	/// Processes the semantic queue up to a certain position
	pub fn process_queue(&self, pos: usize) {
		let mut queue = self.sems.semantic_queue.write();
		while !queue.is_empty() {
			let (range, token) = queue.front().unwrap();
			if range.start > pos {
				break;
			}

			self.add_impl(range.to_owned(), token.to_owned());
			queue.pop_front();
		}
	}

	fn add_impl(&self, range: Range<usize>, token: (u32, u32)) {
		let mut tokens = self.sems.tokens.write();
		let mut cursor = self.sems.cursor.write();
		let mut current = cursor.clone();
		cursor.move_to(range.start);

		while cursor.pos != range.end {
			let end = self.source.content()[cursor.pos..range.end]
				.find('\n')
				.unwrap_or(self.source.content().len() - 1)
				+ 1;
			let len = usize::min(range.end - cursor.pos, end);
			let clen = self.source.content()[cursor.pos..cursor.pos + len]
				.chars()
				.fold(0, |acc, c| acc + c.len_utf16());

			let delta_line = cursor.line - current.line;
			let delta_start = if delta_line == 0 {
				cursor.line_pos - current.line_pos
			} else {
				cursor.line_pos
			};

			tokens.push(SemanticToken {
				delta_line: delta_line as u32,
				delta_start: delta_start as u32,
				length: clen as u32,
				token_type: token.0,
				token_modifiers_bitset: token.1,
			});
			if cursor.pos + len == range.end {
				break;
			}
			current = cursor.clone();
			let pos = cursor.pos;
			cursor.move_to(pos + len);
		}
	}

	/// Add a semantic token to be processed immediately
	///
	/// Note that this will move the cursor to the start of the range, thus making it impossible to add another token before this one.
	/// Use the [`Self::add_to_queue`] if you need to be able to add other tokens before a new token.
	pub fn add(&self, range: Range<usize>, token: (u32, u32)) {
		let range = self.original_source.original_range(range).range;
		self.process_queue(range.start);
		self.add_impl(range, token);
	}

	/// Add a semantic token to be processed in a future call to `add()`
	pub fn add_to_queue(&self, range: Range<usize>, token: (u32, u32)) {
		let range = self.original_source.original_range(range).range;
		let mut queue = self.sems.semantic_queue.write();
		match queue.binary_search_by_key(&range.start, |(range, _)| range.start) {
			Ok(pos) | Err(pos) => queue.insert(pos, (range, token)),
		}
	}
}

#[cfg(test)]
pub mod tests {
	#[macro_export]
	macro_rules! validate_semantics {
		($state:expr, $source:expr, $idx:expr,) => {};
		($state:expr, $source:expr, $idx:expr, $token_name:ident { $($field:ident == $value:expr),* }; $($tail:tt)*) => {{
			let token = $state.shared.lsp
				.as_ref()
				.unwrap()
				.borrow()
				.semantic_data
				.get(&($source as std::sync::Arc<dyn crate::parser::source::Source>))
				.unwrap()
				.tokens
				.borrow()
				[$idx];
			let token_type = $state.shared.lsp
				.as_ref()
				.unwrap()
				.borrow()
				.semantic_tokens
				.$token_name;

			let found_token = (token.token_type, token.token_modifiers_bitset);
			assert!(found_token == token_type, "Invalid token at index {}, expected {}{token_type:#?}, got: {found_token:#?}",
				$idx, stringify!($token_name));

			$(
				let val = &token.$field;
				assert!(*val == $value, "Invalid field {} at index {}, expected {:#?}, found {:#?}",
					stringify!($field),
					$idx,
					$value,
					val);
			)*

			validate_semantics!($state, $source, ($idx+1), $($tail)*);
		}};
		($state:expr, $source:expr, $idx:expr, $token_name:ident; $($tail:tt)*) => {{
			let token = $state.shared.semantics
				.as_ref()
				.unwrap()
				.borrow()
				.sems
				.get(&($source as std::rc::Rc<dyn crate::parser::source::Source>))
				.unwrap()
				.tokens
				.borrow()
				[$idx];
			let token_type = $state.shared.semantics
				.as_ref()
				.unwrap()
				.borrow()
				.tokens
				.$token_name;


			let found_token = (token.token_type, token.token_modifiers_bitset);
			assert!(found_token == token_type, "Invalid token at index {}, expected {}{token_type:#?}, got: {found_token:#?}",
				$idx, stringify!($token_name));

			validate_semantics!($state, $source, ($idx+1), $($tail)*);
		}};
	}
}
