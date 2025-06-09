use core::fmt::Debug;
use std::fs;
use std::ops::Range;
use std::sync::Arc;

use downcast_rs::impl_downcast;
use downcast_rs::Downcast;
use unicode_segmentation::UnicodeSegmentation;

/// Trait for source content
pub trait Source: Downcast + Send + Sync {
	/// Gets the source's location
	///
	/// This usually means the parent source.
	/// If the source is a [`SourceFile`], this generally means the [`SourceFile`] that included it.
	fn location(&self) -> Option<&Token>;
	/// Gets the source's name
	///
	/// For [`SourceFile`] this means the path of the source. Note that some [`VirtualSource`] are prefixed with a special identifier such as `:LUA:`.
	fn name(&self) -> &String;
	/// Gets the source's content
	fn content(&self) -> &String;
}
impl_downcast!(Source);

impl core::fmt::Debug for dyn Source {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.name())
	}
}

impl core::fmt::Display for dyn Source {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.name())
	}
}

impl std::cmp::PartialEq for dyn Source {
	fn eq(&self, other: &Self) -> bool {
		self.name() == other.name()
	}
}

impl std::cmp::Eq for dyn Source {}

impl std::hash::Hash for dyn Source {
	fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
		self.name().hash(state)
	}
}

/// [`SourceFile`] is a type of [`Source`] that represents a real file.
#[derive(Debug)]
pub struct SourceFile {
	/// The token that created this [`SourceFile`], empty if file comes from the executable's
	/// options.
	location: Option<Token>,
	/// Path relative to the compilation databse / current directory
	path: String,
	/// Content of the file
	content: String,
}

impl SourceFile {
	// TODO: Create a SourceFileRegistry holding already loaded files to avoid reloading them
	/// Creates a [`SourceFile`] from a `path`. This will read the content of the file at that
	/// `path`. In case the file is not accessible or reading fails, an error is returned.
	pub fn new(path: String, location: Option<Token>) -> Result<Self, String> {
		match fs::read_to_string(&path) {
			Err(_) => Err(format!("Unable to read file content: `{path}`")),
			Ok(content) => Ok(Self {
				location,
				path: path,
				content,
			}),
		}
	}

	/// Creates a [`SourceFile`] from a `String`
	pub fn with_content(path: String, content: String, location: Option<Token>) -> Self {
		Self {
			location,
			path,
			content,
		}
	}

	/// Gets the path of this [`SourceFile`]
	pub fn path(&self) -> &String {
		&self.path
	}
}

impl Source for SourceFile {
	fn location(&self) -> Option<&Token> {
		self.location.as_ref()
	}
	fn name(&self) -> &String {
		&self.path
	}
	fn content(&self) -> &String {
		&self.content
	}
}

/// Stores the offsets in a virtual source
///
/// The offsets are used to implement the [`SourcePosition`] trait, which allows diagnostics from
/// [`VirtualSource`] to propagate back to their corresponding [`SourceFile`].
///
/// # Example
///
/// Let's say you make a virtual source from the following: "Con\\]tent" -> "Con]tent"
/// Then at position 3, an offset of 1 will be created to account for the removed '\\'
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

/// [`VirtualSource`] is a type of [`Source`] that represents a virtual file. [`VirtualSource`]s
/// can be created from other [`VirtualSource`]s but it must always have a [`SourceFile`] ancestor.
///
///	# Offsets
///
/// [`VirtualSource`] will keep a list of offsets that were applied from the their parent source (available via [`Self::location`]).
/// For instance, if you consider the [`VirtualSource`] created by removing the '\\' from the following string: "He\\llo", then an offset is stored to account for the missing '\\'. This is required in order to keep diagnostics accurate.
#[derive(Debug)]
pub struct VirtualSource {
	/// Token that createrd this [`VirtualSource`]
	location: Token,
	/// Name of the [`VirtualSource`]
	name: String,
	/// Content of the [`VirtualSource`]
	content: String,
	/// Offsets relative to the [`Self::location`]'s source
	offsets: Option<SourceOffset>,
}

impl VirtualSource {
	/// Creates a new [`VirtualSource`] from a `location`, `name` and `content`.
	pub fn new(location: Token, name: String, content: String) -> Self {
		Self {
			location,
			name,
			content,
			offsets: None,
		}
	}

	/// Creates a new [`VirtualSource`] from a `location`, `name`, `content` and `offsets`.
	///
	/// # Notes
	///
	/// This should be called by [`crate::parser::util::escape_source`]
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
	fn location(&self) -> Option<&Token> {
		Some(&self.location)
	}
	fn name(&self) -> &String {
		&self.name
	}
	fn content(&self) -> &String {
		&self.content
	}
}

/// Trait for accessing position in a parent [`SourceFile`]
///
/// This trait is used to create precise error diagnostics and the bakcbone of the LSP.
///
/// # Example
///
/// Given the following source file:
/// ```
/// input.nml:
/// [*link*](url)
/// ```
/// When parsed, a [`VirtualSource`] is created for parsing the link display: `*link*`.
/// If an error or a semantic highlight is requested for that new source, this trait allows to
/// recover the original position in the parent [`SourceFile`].
pub trait SourcePosition {
	/// Transforms a position to the corresponding position in the oldest parent [`SourceFile`].
	///
	/// This function will return the first parent [`SourceFile`] aswell as the position mapped
	/// in that source
	fn original_position(&self, pos: usize) -> (Arc<dyn Source>, usize);

	/// Transforms a range to the corresponding range in the oldest parent [`SourceFile`].
	///
	/// This function will return the first parent [`SourceFile`] aswell as the range mapped
	/// in that source
	fn original_range(&self, range: Range<usize>) -> Token;
}

impl SourcePosition for Arc<dyn Source> {
	fn original_position(&self, mut pos: usize) -> (Arc<dyn Source>, usize) {
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

	fn original_range(&self, mut range: Range<usize>) -> Token {
		// Stop recursion
		if self.downcast_ref::<SourceFile>().is_some() {
			return Token::new(range, self.clone());
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

		Token::new(range, self.clone())
	}
}

/// Cursor in a file
///
/// Represents a position in a specific file.
#[derive(Debug, Clone)]
pub struct Cursor {
	pos: usize,
	source: Arc<dyn Source>,
}

impl Cursor {
	pub fn new(pos: usize, source: Arc<dyn Source>) -> Self {
		Self { pos, source }
	}

	pub fn pos(&self) -> usize {
		self.pos
	}

	pub fn source(&self) -> Arc<dyn Source> {
		self.source.clone()
	}

	/// Creates [`Cursor`] at `new_pos` in the same [`Source`]
	pub fn at(&self, new_pos: usize) -> Self {
		Self {
			pos: new_pos,
			source: self.source.clone(),
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OffsetEncoding {
	Utf8,
	Utf16,
}

/// Cursor type used for the language server
///
/// # Notes
///
/// Because the LSP uses UTF-16 encoded positions, field [`Self::line_pos`] corresponds to the UTF-16
/// distance between the first character (position = 0 or after '\n') and the character at the
/// current position.
#[derive(Debug, Clone)]
pub struct LineCursor {
	/// Byte position in the source
	pub pos: usize,
	/// Line number
	pub line: usize,
	/// Position in the line
	pub line_pos: usize,
	/// Source
	pub source: Arc<dyn Source>,
	/// Offset encoding
	pub encoding: OffsetEncoding,
}

impl LineCursor {
	/// Creates a [`LineCursor`] at the begining of the source
	pub fn new(source: Arc<dyn Source>, offset_encoding: OffsetEncoding) -> LineCursor {
		Self {
			pos: 0,
			line: 0,
			line_pos: 0,
			source,
			encoding: offset_encoding,
		}
	}

	/// Creates a [`LineCursor`] at a given graphical position
	pub fn from_position(
		source: Arc<dyn Source>,
		encoding: OffsetEncoding,
		line: u32,
		column: u32,
	) -> LineCursor {
		let mut cursor = Self::new(source.clone(), encoding);

		while cursor.line < line as usize {
			let left = &cursor.source.content()[cursor.pos..];
			let c = left.chars().next().unwrap();
			cursor.move_to(cursor.pos + c.len_utf8());
		}
		while cursor.line_pos < column as usize {
			let left = &cursor.source.content()[cursor.pos..];
			let c = left.chars().next().unwrap();
			cursor.move_to(cursor.pos + c.len_utf8());
		}
		cursor
		/*
		let mut line_count = 0;
		let mut col_count = 0;
		for (pos, c) in source.content().char_indices() {
			if c == '\n' {
				col_count = 0;
				line_count += 1;
			} else {
				col_count += match cursor.encoding {
					OffsetEncoding::Utf8 => c.len_utf8(),
					OffsetEncoding::Utf16 => c.len_utf16(),
				}
			}
			if line_count == (line + 1) as usize && col_count == (column + 1) as usize {
				cursor.move_to(pos);
				break;
			}
		}
		cursor*/
	}

	/// Moves [`LineCursor`] to an absolute byte position
	/// This function may only advance the position, as is required for the LSP semantics.
	///
	/// # Error
	///
	/// This function will panic if [`Self::pos`] is not UTF-8 aligned, or if trying to go to a previous position.
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
				self.line_pos += match self.encoding {
					OffsetEncoding::Utf8 => c.len_utf8(),
					OffsetEncoding::Utf16 => c.len_utf16(),
				};
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

/// A token is a [`Range<usize>`] in a [`Source`]
#[derive(Debug, Clone)]
pub struct Token {
	pub range: Range<usize>,
	source: Arc<dyn Source>,
}

impl Token {
	/// Creates a new token from a range and a source
	pub fn new(range: Range<usize>, source: Arc<dyn Source>) -> Self {
		Self { range, source }
	}

	/// Retrieve the source of the token
	pub fn source(&self) -> Arc<dyn Source> {
		self.source.clone()
	}

	pub fn content(&self) -> &str {
		&self.source.content().as_str()[self.range.clone()]
	}

	/// Get the start byte position of the token
	pub fn start(&self) -> usize {
		self.range.start
	}

	/// Get the end byte position of the token
	pub fn end(&self) -> usize {
		self.range.end
	}

	/// Get a byte position from a grapheme offset
	///
	/// When in need of diagnostics over a range, use this method instead of adding bytes to `start()`
	/// In case the offsets is out of range, the value of `start()` is returned instead.
	///
	/// # Example
	///
	/// Say you have the following range:
	/// `🚽‍👨TEXT` (🚽‍👨 = 3 + TEXT = 4 codepoints)
	/// Calling [`start_offset(1)`] over this range would give you the byte position of character `T`
	pub fn start_offset(&self, offset: usize) -> usize {
		if offset == 0 {
			return self.start();
		}

		let mut graphemes = self.source.content()[self.range.start..self.range.end]
			.grapheme_indices(true)
			.skip(offset - 1);

		graphemes
			.next()
			.map(|(pos, _)| pos)
			.unwrap_or(self.range.end)
	}

	/// Get a byte position from a grapheme offset
	///
	/// When in need of diagnostics over a range, use this method instead of subtracting bytes from `end()`
	/// In case the offsets is out of range, the value of `end()` is returned instead.
	///
	/// # Example
	///
	/// Say you have the following range:
	/// `TEXT🎅‍🦽` (TEXT = 4 + 🎅‍🦽 = 3 codepoints)
	/// Calling [`end_offset(1)`] over this range would give you the byte position of character `🎅‍🦽`
	pub fn end_offset(&self, offset: usize) -> usize {
		if offset == 0 {
			return self.end();
		}

		let mut graphemes = self.source.content()[0..self.range.end]
			.grapheme_indices(true)
			.rev()
			.skip(offset - 1);

		graphemes
			.next()
			.map(|(pos, _)| pos)
			.unwrap_or(self.range.end)
	}

	/// Creates a virtual source from the token
	pub fn to_source(&self, name: String) -> Arc<dyn Source> {
		Arc::new(VirtualSource {
			location: self.clone(),
			name,
			content: self.content().into(),
			offsets: None,
		})
	}
}

impl From<Arc<dyn Source>> for Token {
	fn from(source: Arc<dyn Source>) -> Self {
		Self {
			range: 0..source.content().len(),
			source,
		}
	}
}

impl From<&Range<Cursor>> for Token {
	fn from(range: &Range<Cursor>) -> Self {
		assert_eq!(&range.start.source, &range.end.source);
		Self {
			range: range.start.pos..range.end.pos,
			source: range.start.source.clone(),
		}
	}
}
