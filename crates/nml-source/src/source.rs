use std::fmt;
use std::ops::Range;

use url::Url;

use crate::Encoding;
use crate::Position;

/// The content of a document, with its location.
///
/// Sources are immutable: an edit produces a new source with a higher [`version`](Self::version).
/// Files use `file://` locations, virtual sources use `virtual://`.
///
/// Lines are terminated by `\n`. A `\r` directly before the `\n` belongs to the line
/// terminator, so it is never counted in a column.
pub struct Source {
	location: Url,
	content: String,
	version: u64,
	/// Byte offset at which every line starts. Always starts with `0`.
	line_starts: Vec<usize>,
}

impl Source {
	pub fn new(location: Url, content: String, version: u64) -> Self {
		let line_starts = std::iter::once(0)
			.chain(content.match_indices('\n').map(|(i, _)| i + 1))
			.collect();
		Self {
			location,
			content,
			version,
			line_starts,
		}
	}

	pub fn location(&self) -> &Url {
		&self.location
	}

	pub fn content(&self) -> &str {
		&self.content
	}

	pub fn version(&self) -> u64 {
		self.version
	}

	/// Number of lines. A trailing newline starts a last, empty line.
	pub fn line_count(&self) -> usize {
		self.line_starts.len()
	}

	/// Byte range of `line`, without its line terminator.
	pub fn line_range(&self, line: usize) -> Option<Range<usize>> {
		let start = *self.line_starts.get(line)?;
		let end = match self.line_starts.get(line + 1) {
			Some(&next) => {
				let end = next - 1;
				if self.content[..end].ends_with('\r') {
					end - 1
				} else {
					end
				}
			}
			None => self.content.len(),
		};
		Some(start..end)
	}

	/// Line and column of a byte offset.
	///
	/// Returns `None` if `offset` is past the end of the content or not on a character
	/// boundary. An offset inside a `\r\n` terminator maps to the end of its line.
	pub fn position(&self, offset: usize, encoding: Encoding) -> Option<Position> {
		if !self.content.is_char_boundary(offset) {
			return None;
		}
		let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
		let range = self.line_range(line)?;
		let column = match encoding {
			Encoding::Utf8 => offset.min(range.end) - range.start,
			Encoding::Utf16 => self.content[range.start..offset.min(range.end)]
				.encode_utf16()
				.count(),
		};
		Some(Position::new(line, column))
	}

	/// Byte offset of a line and column.
	///
	/// Returns `None` if the line does not exist. As the language server protocol requires, a
	/// column past the end of the line maps to the end of the line. A column that falls inside a
	/// character maps to the start of that character.
	pub fn offset(&self, position: Position, encoding: Encoding) -> Option<usize> {
		let range = self.line_range(position.line)?;
		let mut column = 0;
		for (index, c) in self.content[range.clone()].char_indices() {
			column += encoding.width(c);
			if column > position.column {
				return Some(range.start + index);
			}
		}
		Some(range.end)
	}
}

impl fmt::Debug for Source {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Source")
			.field("location", &self.location.as_str())
			.field("version", &self.version)
			.field("len", &self.content.len())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn source(content: &str) -> Source {
		Source::new(
			Url::parse("file:///test.nml").unwrap(),
			content.to_string(),
			0,
		)
	}

	#[test]
	fn line_starts() {
		assert_eq!(source("").line_count(), 1);
		assert_eq!(source("a").line_count(), 1);
		assert_eq!(source("a\n").line_count(), 2);
		assert_eq!(source("a\nb\n\nc").line_count(), 4);
	}

	#[test]
	fn line_ranges_exclude_terminators() {
		let s = source("ab\r\ncd\n\nef");
		assert_eq!(s.line_range(0), Some(0..2));
		assert_eq!(s.line_range(1), Some(4..6));
		assert_eq!(s.line_range(2), Some(7..7));
		assert_eq!(s.line_range(3), Some(8..10));
		assert_eq!(s.line_range(4), None);
	}

	#[test]
	fn positions_utf8() {
		let s = source("ab\ncd\n");
		assert_eq!(s.position(0, Encoding::Utf8), Some(Position::new(0, 0)));
		assert_eq!(s.position(2, Encoding::Utf8), Some(Position::new(0, 2)));
		assert_eq!(s.position(3, Encoding::Utf8), Some(Position::new(1, 0)));
		assert_eq!(s.position(6, Encoding::Utf8), Some(Position::new(2, 0)));
		assert_eq!(s.position(7, Encoding::Utf8), None);
	}

	#[test]
	fn positions_utf16() {
		// 'é' is 2 bytes and 1 UTF-16 unit, '𝄞' is 4 bytes and 2 UTF-16 units.
		let s = source("é𝄞x\n");
		assert_eq!(s.position(2, Encoding::Utf16), Some(Position::new(0, 1)));
		assert_eq!(s.position(6, Encoding::Utf16), Some(Position::new(0, 3)));
		assert_eq!(s.position(6, Encoding::Utf8), Some(Position::new(0, 6)));
		assert_eq!(s.position(1, Encoding::Utf16), None);
	}

	#[test]
	fn position_inside_crlf_is_end_of_line() {
		let s = source("ab\r\ncd");
		assert_eq!(s.position(2, Encoding::Utf8), Some(Position::new(0, 2)));
		assert_eq!(s.position(3, Encoding::Utf8), Some(Position::new(0, 2)));
		assert_eq!(s.position(4, Encoding::Utf8), Some(Position::new(1, 0)));
	}

	#[test]
	fn offsets() {
		let s = source("é𝄞x\r\nyz");
		assert_eq!(s.offset(Position::new(0, 0), Encoding::Utf16), Some(0));
		assert_eq!(s.offset(Position::new(0, 1), Encoding::Utf16), Some(2));
		assert_eq!(s.offset(Position::new(0, 3), Encoding::Utf16), Some(6));
		assert_eq!(s.offset(Position::new(0, 6), Encoding::Utf8), Some(6));
		assert_eq!(s.offset(Position::new(1, 1), Encoding::Utf8), Some(10));
		assert_eq!(s.offset(Position::new(2, 0), Encoding::Utf8), None);
	}

	#[test]
	fn offsets_clamp() {
		let s = source("é𝄞x\r\nyz");
		// Past the end of the line, before the terminator.
		assert_eq!(s.offset(Position::new(0, 100), Encoding::Utf16), Some(7));
		// Inside the surrogate pair of '𝄞'.
		assert_eq!(s.offset(Position::new(0, 2), Encoding::Utf16), Some(2));
		// Inside the bytes of 'é'.
		assert_eq!(s.offset(Position::new(0, 1), Encoding::Utf8), Some(0));
	}

	#[test]
	fn round_trip() {
		let s = source("a é\n𝄞 b\r\n\nlast");
		for encoding in [Encoding::Utf8, Encoding::Utf16] {
			for (offset, _) in s.content().char_indices() {
				if s.content()[offset..].starts_with('\n') && s.content()[..offset].ends_with('\r')
				{
					continue;
				}
				let position = s.position(offset, encoding).unwrap();
				assert_eq!(
					s.offset(position, encoding),
					Some(offset),
					"{offset} {position:?}"
				);
			}
		}
	}
}
