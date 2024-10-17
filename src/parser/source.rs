use core::fmt::Debug;
use std::fs;
use std::ops::Range;
use std::rc::Rc;

use downcast_rs::impl_downcast;
use downcast_rs::Downcast;
use unicode_width::UnicodeWidthChar;

/// Trait for source content
pub trait Source: Downcast {
	/// Gets the source's location
	fn location(&self) -> Option<&Token>;
	/// Gets the source's name
	fn name(&self) -> &String;
	/// Gets the source's content
	fn content(&self) -> &String;
}
impl_downcast!(Source);

impl core::fmt::Display for dyn Source {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}", self.name())
	}
}

impl core::fmt::Debug for dyn Source {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Source{{{}}}", self.name())
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

pub struct SourceFile {
	location: Option<Token>,
	path: String,
	content: String,
}

impl SourceFile {
	// TODO: Create a SourceFileRegistry holding already loaded files to avoid reloading them
	pub fn new(path: String, location: Option<Token>) -> Result<Self, String> {
		match fs::read_to_string(&path) {
			Err(_) => {
				Err(format!(
					"Unable to read file content: `{}`",
					path
				))
			}
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

pub struct VirtualSource {
	location: Token,
	name: String,
	content: String,
}

impl VirtualSource {
	pub fn new(location: Token, name: String, content: String) -> Self {
		Self {
			location,
			name,
			content,
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

#[derive(Debug)]
pub struct Cursor {
	pub pos: usize,
	pub source: Rc<dyn Source>,
}

impl Cursor {
	pub fn new(pos: usize, source: Rc<dyn Source>) -> Self {
		Self { pos, source }
	}

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

	fn clone_from(&mut self, source: &Self) {
		*self = source.clone()
	}
}

/// Cursor type used for the language server
#[derive(Debug, Clone)]
pub struct LineCursor {
	pub pos: usize,
	pub line: usize,
	pub line_pos: usize,
	pub source: Rc<dyn Source>,
}

impl LineCursor {
	/// Creates a [`LineCursor`] at the begining of the source
	pub fn new(source: Rc<dyn Source>) -> LineCursor
	{
		Self {
			pos: 0,
			line: 0,
			line_pos: 0,
			source,
		}
	}

	/// Moves [`LineCursor`] to absolute position
	///
	/// # Error
	/// This function will panic if [`pos`] is not utf8 aligned
	pub fn move_to(&mut self, pos: usize) {
		if self.pos < pos {
			let start = self.pos;
			//eprintln!("slice{{{}}}, want={pos}", &self.source.content().as_str()[start..pos]);
			let mut it = self.source.content().as_str()[start..] // pos+1
				.chars()
				.peekable();

			let mut prev = self.source.content().as_str()[..start]
				.chars()
				.rev()
				.next();
			//eprintln!("prev={prev:#?}");
			while self.pos < pos {
				let c = it.next().unwrap();
				let len = c.len_utf8();

				if self.pos != 0 && prev == Some('\n') {
					self.line += 1;
					self.line_pos = 0;
				}	
				self.line_pos += c.width().unwrap_or(1);
				self.pos += len;

				eprintln!("({}, {c:#?}, {} {}, {})", self.pos, self.line, self.line_pos, prev.unwrap_or(' '));
				prev = Some(c);
			}
			if self.pos != 0 && prev == Some('\n') {
				self.line += 1;
				self.line_pos = 0;
			}
		} else if self.pos > pos {
			panic!("Going back is not supported");
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
	pub fn new(range: Range<usize>, source: Rc<dyn Source>) -> Self {
		Self { range, source }
	}

	pub fn source(&self) -> Rc<dyn Source> {
		self.source.clone()
	}

	/// Construct Token from a range
	pub fn from(start: &Cursor, end: &Cursor) -> Self {
		assert!(Rc::ptr_eq(&start.source, &end.source));

		Self {
			range: start.pos..end.pos,
			source: start.source.clone(),
		}
	}

	pub fn start(&self) -> usize {
		self.range.start
	}

	pub fn end(&self) -> usize {
		self.range.end
	}
}
