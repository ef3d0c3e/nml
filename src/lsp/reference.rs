use std::ops::Range;

/// Reference in the language server
pub struct LsReference {
	/// Reference name
	pub name: String,
	/// Range in the defining document
	pub range: Range<usize>,
	/// Path to defining document
	pub source_path: String,
	/// Refkey of defining document
	pub source_refkey: String,
	/// Type of reference
	pub reftype: String,
}
