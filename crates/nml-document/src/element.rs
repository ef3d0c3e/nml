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
	/// A block of code; its children are the title.
	CodeBlock {
		/// The language to highlight the code in, as written.
		language: Option<String>,
		code: String,
		/// The number of the first line, if lines are numbered.
		first_line: Option<i64>,
	},
	/// An image, a video or an audio file.
	Media {
		kind: MediaKind,
		/// Relative to the page, or absolute.
		url: String,
		/// The text that stands for the medium.
		alt: String,
		/// A CSS width, such as `50%` or `20em`.
		width: Option<String>,
	},
	/// A numbered figure: its first child is the block it shows, its second the caption.
	Figure {
		number: u32,
	},
	/// The caption of a figure; its children are the text.
	Caption,
	/// A quotation; its children are blocks.
	Quote {
		author: Option<String>,
		/// The title of the quoted work.
		cite: Option<String>,
		/// Where the quotation comes from.
		url: Option<String>,
	},
	/// A callout (an admonition); its children are an optional [`Caption`](Self::Caption),
	/// the title, then blocks.
	Callout(CalloutKind),
	/// A layout of its parts: an optional [`Caption`](Self::Caption), the title, then
	/// [`LayoutPart`](Self::LayoutPart)s.
	Layout(LayoutKind),
	/// A part of a layout; its children are blocks.
	LayoutPart,
	/// A table: an optional [`Caption`](Self::Caption), then [`TableRow`](Self::TableRow)s.
	Table {
		/// The alignment of each column, from the separator row.
		alignments: Vec<Alignment>,
		/// How many rows, from the first, are header rows.
		header_rows: usize,
		/// The table's number, if it is a `:table` with a caption or a reference.
		number: Option<u32>,
	},
	/// A row of a table; its children are cells.
	TableRow,
	/// A cell of a table; its children are its content.
	TableCell {
		/// How many columns it spans.
		hspan: u32,
		/// How many rows it spans.
		vspan: u32,
	},
	/// Output written as is, without escaping (HTML for the HTML backend).
	Raw {
		content: String,
		/// Whether it is a block, rather than inline content.
		block: bool,
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
		"code_block",
		"media",
		"figure",
		"caption",
		"quote",
		"callout",
		"layout",
		"layout_part",
		"table",
		"table_row",
		"table_cell",
		"raw",
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
			ElementKind::CodeBlock { .. } => "code_block",
			ElementKind::Media { .. } => "media",
			ElementKind::Figure { .. } => "figure",
			ElementKind::Caption => "caption",
			ElementKind::Quote { .. } => "quote",
			ElementKind::Callout(_) => "callout",
			ElementKind::Layout(_) => "layout",
			ElementKind::LayoutPart => "layout_part",
			ElementKind::Table { .. } => "table",
			ElementKind::TableRow => "table_row",
			ElementKind::TableCell { .. } => "table_cell",
			ElementKind::Raw { .. } => "raw",
			ElementKind::List(_) => "list",
			ElementKind::ListItem { .. } => "list_item",
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKind {
	Image,
	Video,
	Audio,
}

impl MediaKind {
	/// The kind of a file, from its extension.
	pub fn from_path(path: &str) -> Option<Self> {
		let path = path.split(['?', '#']).next().unwrap_or(path);
		let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
		Some(match extension.as_str() {
			"png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif" | "bmp" | "ico" => {
				MediaKind::Image
			}
			"mp4" | "webm" | "mov" | "m4v" | "ogv" => MediaKind::Video,
			"mp3" | "ogg" | "oga" | "flac" | "wav" | "m4a" | "opus" | "aac" => MediaKind::Audio,
			_ => return None,
		})
	}

	pub fn name(self) -> &'static str {
		match self {
			MediaKind::Image => "image",
			MediaKind::Video => "video",
			MediaKind::Audio => "audio",
		}
	}
}

/// The alignment of a table column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Alignment {
	#[default]
	Default,
	Left,
	Center,
	Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayoutKind {
	/// One part, centered.
	Centered,
	/// Parts side by side.
	Split,
	/// One part, hidden until the reader opens it.
	Spoiler,
}

impl LayoutKind {
	pub const ALL: [LayoutKind; 3] = [LayoutKind::Centered, LayoutKind::Split, LayoutKind::Spoiler];

	pub fn name(self) -> &'static str {
		match self {
			LayoutKind::Centered => "centered",
			LayoutKind::Split => "split",
			LayoutKind::Spoiler => "spoiler",
		}
	}

	pub fn from_name(name: &str) -> Option<Self> {
		Self::ALL.into_iter().find(|kind| kind.name() == name)
	}

	/// How many parts the layout has, if it is fixed.
	pub fn parts(self) -> Option<usize> {
		match self {
			LayoutKind::Centered | LayoutKind::Spoiler => Some(1),
			LayoutKind::Split => None,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CalloutKind {
	Note,
	Tip,
	Important,
	Warning,
	Caution,
	Todo,
}

impl CalloutKind {
	pub const ALL: [CalloutKind; 6] = [
		CalloutKind::Note,
		CalloutKind::Tip,
		CalloutKind::Important,
		CalloutKind::Warning,
		CalloutKind::Caution,
		CalloutKind::Todo,
	];

	pub fn name(self) -> &'static str {
		match self {
			CalloutKind::Note => "note",
			CalloutKind::Tip => "tip",
			CalloutKind::Important => "important",
			CalloutKind::Warning => "warning",
			CalloutKind::Caution => "caution",
			CalloutKind::Todo => "todo",
		}
	}

	/// The kind named `name`, in any case: `note`, `NOTE`.
	pub fn from_name(name: &str) -> Option<Self> {
		Self::ALL
			.into_iter()
			.find(|kind| kind.name().eq_ignore_ascii_case(name))
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn media_kinds() {
		for (path, kind) in [
			("a.png", Some(MediaKind::Image)),
			("dir.v2/A.JPG", Some(MediaKind::Image)),
			("https://x.y/clip.webm?t=3#start", Some(MediaKind::Video)),
			("song.flac", Some(MediaKind::Audio)),
			("notes.txt", None),
			("noextension", None),
		] {
			assert_eq!(MediaKind::from_path(path), kind, "{path}");
		}
	}

	#[test]
	fn names() {
		assert_eq!(ElementKind::NAMES.len(), 23);
		assert!(ElementKind::NAMES.contains(&ElementKind::Caption.name()));
	}
}
