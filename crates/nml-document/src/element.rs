use nml_source::Token;

use crate::SlotId;

/// Identifies an element in its [`Document`](crate::Document).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementId(pub(crate) u32);

impl ElementId {
	/// The index of the element in its document, for storage.
	pub fn index(self) -> u32 {
		self.0
	}

	/// The ID of the element at `index`, read back from storage. It is only meaningful for
	/// the document it was taken from.
	pub fn from_index(index: u32) -> Self {
		Self(index)
	}
}

/// What an element is.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ElementKind {
	/// The root of a document.
	Document,
	Paragraph,
	/// Text, with escapes already resolved.
	Text(String),
	/// Styled inline content.
	Style(Style),
	/// A heading; its children are the title.
	Heading {
		level: u8,
	},
	/// Where a slot's value goes.
	Slot(SlotId),
	/// A link to a symbol or a page. Its URL is the `href` slot; its children are the text.
	Reference {
		href: SlotId,
	},
	/// Inline code, verbatim.
	Code(String),
	/// A link to a URL; its children are the text.
	Link {
		url: String,
	},
	/// A list; its children are list items.
	List(ListKind),
	/// An item of a list; its children are blocks.
	ListItem {
		checkbox: Option<Checkbox>,
	},
}

impl ElementKind {
	/// The names of every kind, as [`name`](Self::name) gives them.
	pub const NAMES: &[&str] = &[
		"document",
		"paragraph",
		"text",
		"style",
		"heading",
		"slot",
		"reference",
		"code",
		"link",
		"list",
		"list_item",
	];

	/// The name of the kind, as element events see it (`heading`, `paragraph`, …).
	pub fn name(&self) -> &'static str {
		match self {
			ElementKind::Document => "document",
			ElementKind::Paragraph => "paragraph",
			ElementKind::Text(_) => "text",
			ElementKind::Style(_) => "style",
			ElementKind::Heading { .. } => "heading",
			ElementKind::Slot(_) => "slot",
			ElementKind::Reference { .. } => "reference",
			ElementKind::Code(_) => "code",
			ElementKind::Link { .. } => "link",
			ElementKind::List(_) => "list",
			ElementKind::ListItem { .. } => "list_item",
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListKind {
	Bullet,
	/// Numbered from `start`.
	Numbered {
		start: u64,
	},
}

/// The state of a list item's checkbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Checkbox {
	Unchecked,
	/// Partly done: some of the item's subitems are.
	Partial,
	Checked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Style {
	Bold,
	Italic,
	Underline,
	Highlight,
	Strikethrough,
}

/// The fields of an element that element events may change before it is written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attributes {
	/// The reference name (`{#name}`), which also becomes the HTML `id`.
	pub id: Option<String>,
	pub classes: Vec<String>,
}

/// An element of the tree. Once written to a [`Document`](crate::Document), it does not
/// change.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
	pub kind: ElementKind,
	/// Where the element comes from.
	pub token: Token,
	pub attributes: Attributes,
	pub(crate) parent: Option<ElementId>,
	pub(crate) children: Vec<ElementId>,
}

impl Element {
	pub fn parent(&self) -> Option<ElementId> {
		self.parent
	}

	pub fn children(&self) -> &[ElementId] {
		&self.children
	}
}
