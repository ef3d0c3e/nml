/// Unit in which columns are counted.
///
/// The language server protocol counts columns in UTF-16 code units by default, while the
/// rest of NML works with UTF-8 byte offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Encoding {
	Utf8,
	Utf16,
}

impl Encoding {
	/// Width of `c` in this encoding's code units.
	pub(crate) fn width(self, c: char) -> usize {
		match self {
			Encoding::Utf8 => c.len_utf8(),
			Encoding::Utf16 => c.len_utf16(),
		}
	}
}

/// A zero-based line and column in a [`Source`](crate::Source).
///
/// The column is counted in the units of the [`Encoding`] used to compute the position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
	pub line: usize,
	pub column: usize,
}

impl Position {
	pub fn new(line: usize, column: usize) -> Self {
		Self { line, column }
	}
}
