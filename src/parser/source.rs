use core::fmt::Debug;
use std::fs;
use std::ops::Range;
use std::rc::Rc;

use downcast_rs::impl_downcast;
use downcast_rs::Downcast;

/// Trait for source content
pub trait Source: Downcast + Debug {
	/// Gets the source's location
	fn location(&self) -> Option<&Token>;
	/// Gets the source's name
	fn name(&self) -> &String;
	/// Gets the source's content
	fn content(&self) -> &String;
}
impl_downcast!(Source);

pub trait SourcePosition {
	/// Transforms a position to it's position in the oldest parent source
	fn original_position(&self, pos: usize) -> (Rc<dyn Source>, usize);

	/// Transforms a range to the oldest parent source
	///
	/// This function takes a range from a source and attempts to get the range's position in the oldest parent
	fn original_range(&self, range: Range<usize>) -> (Rc<dyn Source>, Range<usize>);
}

impl core::fmt::Display for dyn Source {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.name())
	}
}

impl std::cmp::PartialEq for dyn Source {
	fn eq(&self, other: &Self) -> bool { self.name() == other.name() }
}

impl std::cmp::Eq for dyn Source {}

impl std::hash::Hash for dyn Source {
	fn hash<H: std::hash::Hasher>(&self, state: &mut H) { self.name().hash(state) }
}

#[derive(Debug)]
pub struct SourceFile {
	location: Option<Token>,
	path: String,
	content: String,
}

impl SourceFile {
	// TODO: Create a SourceFileRegistry holding already loaded files to avoid reloading them
	pub fn new(path: String, location: Option<Token>) -> Result<Self, String> {
		match fs::read_to_string(&path) {
			Err(_) => Err(format!("Unable to read file content: `{}`", path)),
			Ok(content) => Ok(Self {
				location,
				path,
				content,
			}),
		}
	}

	pub fn with_content(path: String, content: String, location: Option<Token>) -> Self {
		Self {
			location,
			path,
			content,
		}
	}

	pub fn path(&self) -> &String { &self.path }
}

impl Source for SourceFile {
	fn location(&self) -> Option<&Token> { self.location.as_ref() }
	fn name(&self) -> &String { &self.path }
	fn content(&self) -> &String { &self.content }
}

/// Stores the offsets in a virtual source
///
/// # Example
///
/// Let's say you make a virtual source from the following: "Con\]tent" -> "Con]tent"
/// Then at position 3, an offset of 1 will be created to account for the removed '\'
#[derive(Debug)]
struct SourceOffset {
	/// Stores the total offsets
	offsets: Vec<(usize, isize)>,
}

impl SourceOffset {
	/// Get the offset position
	pub fn position(&self, pos: usize) -> usize {
		match self.offsets.binary_search_by_key(&pos, |&(orig, _)| orig) {
			Ok(idx) => (pos as isize + self.offsets[idx].1) as usize,
			Err(idx) => {
				if idx == 0 {
					pos
				} else {
					(pos as isize + self.offsets[idx - 1].1) as usize
				}
			}
		}
	}
}

#[derive(Debug)]
pub struct VirtualSource {
	location: Token,
	name: String,
	content: String,
	/// Offset relative to the [`location`]'s source
	offsets: Option<SourceOffset>,
}

impl VirtualSource {
	pub fn new(location: Token, name: String, content: String) -> Self {
		Self {
			location,
			name,
			content,
			offsets: None,
		}
	}

	pub fn new_offsets(
		location: Token,
		name: String,
		content: String,
		offsets: Vec<(usize, isize)>,
	) -> Self {
		Self {
			location,
			name,
			content,
			offsets: Some(SourceOffset { offsets }),
		}
	}
}

impl Source for VirtualSource {
	fn location(&self) -> Option<&Token> { Some(&self.location) }
	fn name(&self) -> &String { &self.name }
	fn content(&self) -> &String { &self.content }
}

impl SourcePosition for Rc<dyn Source> {
	fn original_position(&self, mut pos: usize) -> (Rc<dyn Source>, usize) {
		// Stop recursion
		if self.downcast_ref::<SourceFile>().is_some() {
			return (self.clone(), pos);
		}

		// Apply offsets
		if let Some(offsets) = self
			.downcast_ref::<VirtualSource>()
			.and_then(|source| source.offsets.as_ref())
		{
			pos = offsets.position(pos);
		}

		// Recurse to parent
		if let Some(parent) = self.location() {
			return parent.source().original_position(parent.range.start + pos);
		}

		(self.clone(), pos)
	}

	fn original_range(&self, mut range: Range<usize>) -> (Rc<dyn Source>, Range<usize>) {
		// Stop recursion
		if self.downcast_ref::<SourceFile>().is_some() {
			return (self.clone(), range);
		}

		// Apply offsets
		if let Some(offsets) = self
			.downcast_ref::<VirtualSource>()
			.and_then(|source| source.offsets.as_ref())
		{
			range = offsets.position(range.start)..offsets.position(range.end);
		}

		// Recurse to parent
		if let Some(parent) = self.location() {
			return parent
				.source
				.original_range(parent.range.start + range.start..parent.range.start + range.end);
		}

		(self.clone(), range)
	}
}

#[derive(Debug)]
pub struct Cursor {
	pub pos: usize,
	pub source: Rc<dyn Source>,
}

impl Cursor {
	pub fn new(pos: usize, source: Rc<dyn Source>) -> Self { Self { pos, source } }

	/// Creates [`cursor`] at [`new_pos`] in the same [`file`]
	pub fn at(&self, new_pos: usize) -> Self {
		Self {
			pos: new_pos,
			source: self.source.clone(),
		}
	}
}

impl Clone for Cursor {
	fn clone(&self) -> Self {
		Self {
			pos: self.pos,
			source: self.source.clone(),
		}
	}

	fn clone_from(&mut self, source: &Self) { *self = source.clone() }
}

/// Cursor type used for the language server
#[derive(Debug, Clone)]
pub struct LineCursor {
	/// Byte position in the source
	pub pos: usize,
	/// Line number
	pub line: usize,
	/// Position in the line
	pub line_pos: usize,
	/// Source
	pub source: Rc<dyn Source>,
}

impl LineCursor {
	/// Creates a [`LineCursor`] at the begining of the source
	pub fn new(source: Rc<dyn Source>) -> LineCursor {
		Self {
			pos: 0,
			line: 0,
			line_pos: 0,
			source,
		}
	}

	/// Moves [`LineCursor`] to an absolute byte position
	///
	/// # Error
	/// This function will panic if [`pos`] is not utf8 aligned
	pub fn move_to(&mut self, pos: usize) {
		if self.pos < pos {
			let start = self.pos;
			let mut it = self.source.content().as_str()[start..].chars().peekable();

			let mut prev = self.source.content().as_str()[..start].chars().next_back();
			while self.pos < pos {
				let c = it.next().unwrap();

				if self.pos != start && prev == Some('\n') {
					self.line += 1;
					self.line_pos = 0;
				}
				self.line_pos += c.len_utf16();
				self.pos += c.len_utf8();
				prev = Some(c);
			}
			if self.pos != start && prev == Some('\n') {
				self.line += 1;
				self.line_pos = 0;
			}
		} else if self.pos > pos {
			panic!();
		}

		// May fail if pos is not utf8-aligned
		assert_eq!(pos, self.pos);
	}
}

#[derive(Debug, Clone)]
pub struct Token {
	pub range: Range<usize>,
	source: Rc<dyn Source>,
}

impl Token {
	pub fn new(range: Range<usize>, source: Rc<dyn Source>) -> Self { Self { range, source } }

	pub fn source(&self) -> Rc<dyn Source> { self.source.clone() }

	/// Construct Token from a range
	pub fn from(start: &Cursor, end: &Cursor) -> Self {
		assert!(Rc::ptr_eq(&start.source, &end.source));

		Self {
			range: start.pos..end.pos,
			source: start.source.clone(),
		}
	}

	pub fn start(&self) -> usize { self.range.start }

	pub fn end(&self) -> usize { self.range.end }
}
