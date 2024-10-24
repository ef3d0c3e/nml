use std::cell::Ref;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use tower_lsp::lsp_types::SemanticToken;
use tower_lsp::lsp_types::SemanticTokenModifier;
use tower_lsp::lsp_types::SemanticTokenType;

use crate::parser::source::LineCursor;
use crate::parser::source::Source;
use crate::parser::source::SourceFile;
use crate::parser::source::SourcePosition;
use crate::parser::source::VirtualSource;

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
	pub section_heading: (u32, u32),
	pub section_reference: (u32, u32),
	pub section_kind: (u32, u32),
	pub section_name: (u32, u32),

	pub comment: (u32, u32),

	pub link_display_sep: (u32, u32),
	pub link_url_sep: (u32, u32),
	pub link_url: (u32, u32),

	pub style_marker: (u32, u32),

	pub import_import: (u32, u32),
	pub import_as_sep: (u32, u32),
	pub import_as: (u32, u32),
	pub import_path: (u32, u32),

	pub reference_operator: (u32, u32),
	pub reference_link_sep: (u32, u32),
	pub reference_doc_sep: (u32, u32),
	pub reference_doc: (u32, u32),
	pub reference_link: (u32, u32),
	pub reference_props_sep: (u32, u32),
	pub reference_props: (u32, u32),

	pub variable_operator: (u32, u32),
	pub variable_kind: (u32, u32),
	pub variable_name: (u32, u32),
	pub variable_sep: (u32, u32),
	pub variable_value: (u32, u32),

	pub variable_sub_sep: (u32, u32),
	pub variable_sub_name: (u32, u32),

	pub code_sep: (u32, u32),
	pub code_props_sep: (u32, u32),
	pub code_props: (u32, u32),
	pub code_lang: (u32, u32),
	pub code_title: (u32, u32),
	pub code_content: (u32, u32),

	pub script_sep: (u32, u32),
	pub script_kernel_sep: (u32, u32),
	pub script_kernel: (u32, u32),
	pub script_kind: (u32, u32),
	pub script_content: (u32, u32),

	pub list_bullet: (u32, u32),
	pub list_props_sep: (u32, u32),
	pub list_props: (u32, u32),

	pub raw_sep: (u32, u32),
	pub raw_props_sep: (u32, u32),
	pub raw_props: (u32, u32),
	pub raw_content: (u32, u32),

	pub tex_sep: (u32, u32),
	pub tex_props_sep: (u32, u32),
	pub tex_props: (u32, u32),
	pub tex_content: (u32, u32),

	pub layout_sep: (u32, u32),
	pub layout_token: (u32, u32),
	pub layout_props_sep: (u32, u32),
	pub layout_props: (u32, u32),
	pub layout_type: (u32, u32),
}

impl Tokens {
	pub fn new() -> Self {
		Self {
			section_heading: token!("number"),
			section_reference: token!("enum", "async"),
			section_kind: token!("enum"),
			section_name: token!("string"),

			comment: token!("comment"),

			link_display_sep: token!("macro"),
			link_url_sep: token!("macro"),
			link_url: token!("function", "readonly", "abstract", "abstract"),

			style_marker: token!("operator"),

			import_import: token!("macro"),
			import_as_sep: token!("operator"),
			import_as: token!("operator"),
			import_path: token!("parameter"),

			reference_operator: token!("operator"),
			reference_link_sep: token!("operator"),
			reference_doc_sep: token!("function"),
			reference_doc: token!("function"),
			reference_link: token!("macro"),
			reference_props_sep: token!("operator"),
			reference_props: token!("enum"),

			variable_operator: token!("operator"),
			variable_kind: token!("operator"),
			variable_name: token!("macro"),
			variable_sep: token!("operator"),
			variable_value: token!("parameter"),

			variable_sub_sep: token!("operator"),
			variable_sub_name: token!("macro"),

			code_sep: token!("operator"),
			code_props_sep: token!("operator"),
			code_props: token!("enum"),
			code_lang: token!("function"),
			code_title: token!("number"),
			code_content: token!("string"),

			script_sep: token!("operator"),
			script_kernel_sep: token!("operator"),
			script_kernel: token!("function"),
			script_kind: token!("function"),
			script_content: token!("string"),

			list_bullet: token!("macro"),
			list_props_sep: token!("operator"),
			list_props: token!("enum"),

			raw_sep: token!("operator"),
			raw_props_sep: token!("operator"),
			raw_props: token!("enum"),
			raw_content: token!("string"),

			tex_sep: token!("modifier"),
			tex_props_sep: token!("operator"),
			tex_props: token!("enum"),
			tex_content: token!("string"),

			layout_sep: token!("number"),
			layout_token: token!("number"),
			layout_props_sep: token!("operator"),
			layout_props: token!("enum"),
			layout_type: token!("function"),
		}
	}
}

/// Per file semantic tokens
#[derive(Debug)]
pub struct SemanticsData {
	/// The current cursor
	cursor: RefCell<LineCursor>,

	/// Semantic tokens
	pub tokens: RefCell<Vec<SemanticToken>>,
}

impl SemanticsData {
	pub fn new(source: Rc<dyn Source>) -> Self {
		Self {
			cursor: RefCell::new(LineCursor::new(source)),
			tokens: RefCell::new(vec![]),
		}
	}
}

#[derive(Debug)]
pub struct Semantics<'a> {
	pub(self) sems: Ref<'a, SemanticsData>,
	// TODO
	pub(self) original_source: Rc<dyn Source>,
	/// The resolved parent source
	pub(self) source: Rc<dyn Source>,
}

impl<'a> Semantics<'a> {
	fn from_source_impl(
		source: Rc<dyn Source>,
		semantics: &'a Option<RefCell<SemanticsHolder>>,
		original_source: Rc<dyn Source>,
	) -> Option<(Self, Ref<'a, Tokens>)> {
		if source.name().starts_with(":LUA:") && source.downcast_ref::<VirtualSource>().is_some() {
			return None;
		}

		if let Some(location) = source
			.clone()
			.downcast_rc::<VirtualSource>()
			.ok()
			.as_ref()
			.map(|parent| parent.location())
			.unwrap_or(None)
		{
			return Self::from_source_impl(location.source(), semantics, original_source);
		} else if let Ok(source) = source.clone().downcast_rc::<SourceFile>() {
			return Ref::filter_map(
				semantics.as_ref().unwrap().borrow(),
				|semantics: &SemanticsHolder| {
					semantics.sems.get(&(source.clone() as Rc<dyn Source>))
				},
			)
			.ok()
			.map(|sems| {
				(
					Self {
						sems,
						source,
						original_source,
					},
					Ref::map(
						semantics.as_ref().unwrap().borrow(),
						|semantics: &SemanticsHolder| &semantics.tokens,
					),
				)
			});
		}
		None
	}

	pub fn from_source(
		source: Rc<dyn Source>,
		semantics: &'a Option<RefCell<SemanticsHolder>>,
	) -> Option<(Self, Ref<'a, Tokens>)> {
		if semantics.is_none() {
			return None;
		}
		Self::from_source_impl(source.clone(), semantics, source)
	}

	pub fn add(&self, range: Range<usize>, token: (u32, u32)) {
		let range = self.original_source.original_range(range).1;
		let mut tokens = self.sems.tokens.borrow_mut();
		let mut cursor = self.sems.cursor.borrow_mut();
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
}

#[derive(Debug)]
pub struct SemanticsHolder {
	pub tokens: Tokens,
	pub sems: HashMap<Rc<dyn Source>, SemanticsData>,
}

impl SemanticsHolder {
	pub fn new() -> Self {
		Self {
			tokens: Tokens::new(),
			sems: HashMap::new(),
		}
	}
}

#[cfg(test)]
pub mod tests {
	#[macro_export]
	macro_rules! validate_semantics {
		($state:expr, $source:expr, $idx:expr,) => {};
		($state:expr, $source:expr, $idx:expr, $token_name:ident { $($field:ident == $value:expr),* }; $($tail:tt)*) => {{
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
