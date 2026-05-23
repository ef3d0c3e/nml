use std::ops::Range;
use std::sync::Arc;

use url::Url;

use crate::Source;
use crate::Token;

/// Which side of a piece boundary an offset maps to.
///
/// Where two pieces of a virtual source meet, one virtual offset is both the end of the first
/// piece and the start of the second, which can be far apart in the parent (e.g. with the
/// `> ` prefix of a callout line removed in between). The start of a range should map into the
/// piece that follows ([`Bias::Start`]), and the end of a range into the piece that precedes
/// ([`Bias::End`]), so that the mapped range does not grow to cover the removed text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bias {
	Start,
	End,
}

/// How a virtual source was cut from its parent.
#[derive(Debug)]
pub(crate) struct Parent {
	pub(crate) source: Arc<Source>,
	segments: Vec<Segment>,
}

/// A contiguous run of the virtual content.
///
/// A copied segment maps one to one onto the parent from `parent_start`. An inserted segment
/// has no counterpart in the parent: all of its offsets map to `parent_start`.
#[derive(Debug, Clone, Copy)]
struct Segment {
	virtual_start: usize,
	parent_start: usize,
	copied: bool,
}

/// Builds a virtual source from pieces of a parent source.
///
/// Pieces are copied in order, and copied ranges must not go backwards in the parent. Text that
/// does not come from the parent (e.g. a separator between two Lua blocks) can be inserted; it
/// maps to the parent position where it was inserted.
///
/// ```
/// # use std::sync::Arc;
/// # use nml_source::{Source, Url, VirtualSourceBuilder};
/// let url = Url::parse("file:///a.nml").unwrap();
/// let file = Arc::new(Source::new(url, "> first\n> second\n".into(), 0));
/// let url = Url::parse("virtual:///a.nml/callout").unwrap();
/// let mut builder = VirtualSourceBuilder::new(file, url, 2);
/// builder.copy(2..8);
/// builder.copy(10..17);
/// let callout = builder.build();
/// assert_eq!(callout.content(), "first\nsecond\n");
/// ```
#[derive(Debug)]
pub struct VirtualSourceBuilder {
	parent: Arc<Source>,
	location: Url,
	content: String,
	segments: Vec<Segment>,
	/// Parent offset reached so far: where inserted text maps to.
	cursor: usize,
}

impl VirtualSourceBuilder {
	/// Starts a virtual source of `parent`. Text inserted before the first copy maps to `start`.
	pub fn new(parent: Arc<Source>, location: Url, start: usize) -> Self {
		assert!(
			start <= parent.content().len(),
			"start {start} is past the end of {parent:?}"
		);
		Self {
			parent,
			location,
			content: String::new(),
			segments: Vec::new(),
			cursor: start,
		}
	}

	/// Appends `range` of the parent.
	///
	/// # Panics
	///
	/// If `range` is not a valid range of the parent, or starts before the end of the previous
	/// copy (or before `start` for the first copy).
	pub fn copy(&mut self, range: Range<usize>) -> &mut Self {
		let parent = self.parent.clone();
		let text = parent
			.content()
			.get(range.clone())
			.unwrap_or_else(|| panic!("invalid range {range:?} for {parent:?}"));
		assert!(
			range.start >= self.cursor,
			"range {range:?} goes backwards in the parent"
		);
		if !text.is_empty() {
			self.push(text, range.start, true);
		}
		self.cursor = range.end;
		self
	}

	/// Appends text that is not part of the parent.
	pub fn insert(&mut self, text: &str) -> &mut Self {
		if !text.is_empty() {
			self.push(text, self.cursor, false);
		}
		self
	}

	fn push(&mut self, text: &str, parent_start: usize, copied: bool) {
		self.segments.push(Segment {
			virtual_start: self.content.len(),
			parent_start,
			copied,
		});
		self.content.push_str(text);
	}

	/// Builds the virtual source. It has the parent's version.
	pub fn build(self) -> Source {
		let mut segments = self.segments;
		if segments.is_empty() {
			// An empty virtual source still needs to map offset 0 somewhere.
			segments.push(Segment {
				virtual_start: 0,
				parent_start: self.cursor,
				copied: false,
			});
		}
		let version = self.parent.version();
		Source::with_parent(
			self.location,
			self.content,
			version,
			Some(Parent {
				source: self.parent,
				segments,
			}),
		)
	}
}

impl Parent {
	fn segment_end(&self, index: usize, content_len: usize) -> usize {
		self.segments
			.get(index + 1)
			.map_or(content_len, |next| next.virtual_start)
	}

	fn map_to_parent(&self, offset: usize, bias: Bias, content_len: usize) -> usize {
		let mut index = self
			.segments
			.partition_point(|segment| segment.virtual_start <= offset)
			- 1;
		if bias == Bias::End && index > 0 && self.segments[index].virtual_start == offset {
			index -= 1;
		}
		let segment = self.segments[index];
		if segment.copied {
			let end = self.segment_end(index, content_len);
			segment.parent_start + (offset.min(end) - segment.virtual_start)
		} else {
			segment.parent_start
		}
	}

	fn map_from_parent(&self, offset: usize, content: &str) -> Option<usize> {
		let candidates = self
			.segments
			.partition_point(|segment| segment.parent_start <= offset);
		(0..candidates).rev().find_map(|index| {
			let segment = self.segments[index];
			let text = &content[segment.virtual_start..self.segment_end(index, content.len())];
			// The end of a piece is a valid position in it (e.g. a cursor at the end of a line),
			// unless the piece ends with a newline: that offset is the start of the next line in
			// the parent, which may be removed text.
			let inside = offset < segment.parent_start + text.len()
				|| (offset == segment.parent_start + text.len() && !text.ends_with('\n'));
			(segment.copied && inside)
				.then(|| segment.virtual_start + (offset - segment.parent_start))
		})
	}
}

impl Source {
	/// The source this virtual source was cut from, `None` for a file.
	pub fn parent(&self) -> Option<&Arc<Source>> {
		self.parent.as_ref().map(|parent| &parent.source)
	}

	/// Maps an offset of this source to its parent. A file maps to itself.
	///
	/// # Panics
	///
	/// If `offset` is past the end of the content.
	pub fn offset_in_parent(&self, offset: usize, bias: Bias) -> usize {
		assert!(
			offset <= self.content().len(),
			"offset {offset} out of range"
		);
		match &self.parent {
			Some(parent) => parent.map_to_parent(offset, bias, self.content().len()),
			None => offset,
		}
	}

	/// Maps an offset of the parent into this source.
	///
	/// Returns `None` if the offset is not in a part copied from the parent, such as a removed
	/// line prefix. The end of a copied piece belongs to that piece, unless the piece ends with
	/// a newline. A file maps to itself.
	pub fn offset_from_parent(&self, offset: usize) -> Option<usize> {
		match &self.parent {
			Some(parent) => parent.map_from_parent(offset, self.content()),
			None => (offset <= self.content().len()).then_some(offset),
		}
	}

	/// The file at the end of the parent chain, and `offset` mapped into it.
	pub fn root_offset(self: &Arc<Self>, offset: usize, bias: Bias) -> (Arc<Source>, usize) {
		let mut source = self.clone();
		let mut offset = offset;
		while let Some(parent) = source.parent().cloned() {
			offset = source.offset_in_parent(offset, bias);
			source = parent;
		}
		(source, offset)
	}

	/// Maps an offset of the root file into this source, through every level of the chain.
	///
	/// Returns `None` if some level does not contain the offset.
	pub fn offset_from_root(&self, offset: usize) -> Option<usize> {
		match self.parent() {
			Some(parent) => self.offset_from_parent(parent.offset_from_root(offset)?),
			None => self.offset_from_parent(offset),
		}
	}
}

impl Token {
	/// This token mapped to the file at the end of its source's parent chain.
	pub fn root(&self) -> Token {
		let (source, start) = self.source().root_offset(self.start(), Bias::Start);
		let (_, end) = self.source().root_offset(self.end(), Bias::End);
		Token::new(source, start..end.max(start))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn file(content: &str) -> Arc<Source> {
		Arc::new(Source::new(
			Url::parse("file:///test.nml").unwrap(),
			content.to_string(),
			3,
		))
	}

	fn location(name: &str) -> Url {
		Url::parse(&format!("virtual:///test.nml/{name}")).unwrap()
	}

	/// The body of a callout: "> " is stripped from both lines.
	fn callout() -> (Arc<Source>, Arc<Source>) {
		let file = file("text\n> first\n> second\nafter");
		let mut builder = VirtualSourceBuilder::new(file.clone(), location("callout"), 7);
		builder.copy(7..13).copy(15..22);
		(file, Arc::new(builder.build()))
	}

	#[test]
	fn content_and_metadata() {
		let (file, callout) = callout();
		assert_eq!(callout.content(), "first\nsecond\n");
		assert_eq!(callout.version(), 3);
		assert!(Arc::ptr_eq(callout.parent().unwrap(), &file));
		assert!(file.parent().is_none());
	}

	#[test]
	fn maps_to_parent() {
		let (_, callout) = callout();
		// 'f' of "first"
		assert_eq!(callout.offset_in_parent(0, Bias::Start), 7);
		// 's' of "second"
		assert_eq!(callout.offset_in_parent(6, Bias::Start), 15);
		// End of content
		assert_eq!(callout.offset_in_parent(13, Bias::End), 22);
	}

	#[test]
	fn bias_at_piece_boundary() {
		let (_, callout) = callout();
		assert_eq!(callout.offset_in_parent(6, Bias::Start), 15);
		assert_eq!(callout.offset_in_parent(6, Bias::End), 13);
	}

	#[test]
	fn token_root_does_not_cover_removed_prefix() {
		let (_, callout) = callout();
		let first_line = Token::new(callout.clone(), 0..6);
		assert_eq!(first_line.root().range(), 7..13);
		assert_eq!(first_line.root().content(), "first\n");
		let second = Token::new(callout, 6..12);
		assert_eq!(second.root().content(), "second");
	}

	#[test]
	fn maps_from_parent() {
		let (_, callout) = callout();
		assert_eq!(callout.offset_from_parent(7), Some(0));
		assert_eq!(callout.offset_from_parent(15), Some(6));
		// The first piece ends with a newline: its end is the stripped "> " of the next line.
		assert_eq!(callout.offset_from_parent(13), None);
		// The stripped "> " of the second line.
		assert_eq!(callout.offset_from_parent(14), None);
		// Outside the callout.
		assert_eq!(callout.offset_from_parent(2), None);
		assert_eq!(callout.offset_from_parent(24), None);
	}

	#[test]
	fn inserted_text_maps_to_insertion_point() {
		let file = file("a = 1\nb = 2");
		let mut builder = VirtualSourceBuilder::new(file.clone(), location("lua"), 0);
		builder.copy(0..5).insert("\n-- next block\n").copy(6..11);
		let lua = builder.build();
		assert_eq!(lua.content(), "a = 1\n-- next block\nb = 2");
		assert_eq!(lua.offset_in_parent(8, Bias::Start), 5);
		assert_eq!(lua.offset_in_parent(20, Bias::Start), 6);
		assert_eq!(lua.offset_from_parent(6), Some(20));
		// End of "a = 1": a cursor at the end of the line stays in the first block.
		assert_eq!(lua.offset_from_parent(5), Some(5));
	}

	#[test]
	fn nested_virtual_sources() {
		// A callout inside a callout: the inner one strips another "> ".
		let file = file("> > deep\n> > er");
		let mut outer = VirtualSourceBuilder::new(file.clone(), location("outer"), 2);
		outer.copy(2..9).copy(11..15);
		let outer = Arc::new(outer.build());
		assert_eq!(outer.content(), "> deep\n> er");
		let mut inner = VirtualSourceBuilder::new(outer.clone(), location("inner"), 2);
		inner.copy(2..7).copy(9..11);
		let inner = Arc::new(inner.build());
		assert_eq!(inner.content(), "deep\ner");

		let (root, offset) = inner.root_offset(5, Bias::Start);
		assert!(Arc::ptr_eq(&root, &file));
		assert_eq!(offset, 13);
		assert_eq!(
			Token::new(inner.clone(), 0..7).root().content(),
			"deep\n> > er"
		);
		assert_eq!(inner.offset_from_root(13), Some(5));
		assert_eq!(inner.offset_from_root(11), None);
		assert_eq!(inner.offset_from_root(4), Some(0));
	}

	#[test]
	fn empty_virtual_source() {
		let file = file("abc");
		let empty = Arc::new(VirtualSourceBuilder::new(file, location("empty"), 2).build());
		assert_eq!(empty.content(), "");
		assert_eq!(empty.offset_in_parent(0, Bias::Start), 2);
		assert_eq!(Token::new(empty, 0..0).root().range(), 2..2);
	}

	#[test]
	fn files_map_to_themselves() {
		let file = file("abc");
		assert_eq!(file.offset_in_parent(2, Bias::Start), 2);
		assert_eq!(file.offset_from_parent(3), Some(3));
		assert_eq!(file.offset_from_root(4), None);
		assert_eq!(Token::new(file, 1..2).root().content(), "b");
	}

	#[test]
	#[should_panic(expected = "goes backwards")]
	fn copies_must_go_forward() {
		let file = file("abcdef");
		let mut builder = VirtualSourceBuilder::new(file, location("bad"), 0);
		builder.copy(3..5).copy(1..2);
	}
}
