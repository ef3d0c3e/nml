use std::fmt;
use std::hash::Hash;
use std::hash::Hasher;
use std::ops::Range;
use std::sync::Arc;

use crate::Source;

/// A byte range in a [`Source`].
///
/// Two tokens are equal when they cover the same range of the same source instance, not
/// merely sources with equal content.
#[derive(Clone)]
pub struct Token {
	source: Arc<Source>,
	range: Range<usize>,
}

impl Token {
	/// # Panics
	///
	/// If `range` is reversed, past the end of the source, or not on character boundaries.
	pub fn new(source: Arc<Source>, range: Range<usize>) -> Self {
		assert!(
			source.content().get(range.clone()).is_some(),
			"invalid token range {range:?} for {source:?}"
		);
		Self { source, range }
	}

	pub fn source(&self) -> &Arc<Source> {
		&self.source
	}

	pub fn range(&self) -> Range<usize> {
		self.range.clone()
	}

	pub fn start(&self) -> usize {
		self.range.start
	}

	pub fn end(&self) -> usize {
		self.range.end
	}

	pub fn content(&self) -> &str {
		&self.source.content()[self.range.clone()]
	}
}

impl PartialEq for Token {
	fn eq(&self, other: &Self) -> bool {
		Arc::ptr_eq(&self.source, &other.source) && self.range == other.range
	}
}

impl Eq for Token {}

impl Hash for Token {
	fn hash<H: Hasher>(&self, state: &mut H) {
		Arc::as_ptr(&self.source).hash(state);
		self.range.hash(state);
	}
}

impl fmt::Debug for Token {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}@{:?}", self.source.location(), self.range)
	}
}

#[cfg(test)]
mod tests {
	use url::Url;

	use super::*;

	fn source(content: &str) -> Arc<Source> {
		Arc::new(Source::new(
			Url::parse("file:///test.nml").unwrap(),
			content.to_string(),
			0,
		))
	}

	#[test]
	fn content() {
		let token = Token::new(source("hello world"), 6..11);
		assert_eq!(token.content(), "world");
	}

	#[test]
	fn equality_is_per_source_instance() {
		let a = source("same");
		let b = source("same");
		assert_eq!(Token::new(a.clone(), 0..2), Token::new(a.clone(), 0..2));
		assert_ne!(Token::new(a.clone(), 0..2), Token::new(a, 0..3));
		assert_ne!(Token::new(source("same"), 0..2), Token::new(b, 0..2));
	}

	#[test]
	#[should_panic]
	fn rejects_split_characters() {
		Token::new(source("é"), 0..1);
	}
}
