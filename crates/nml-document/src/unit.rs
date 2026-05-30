use std::fmt;

use nml_source::Token;

use crate::Document;
use crate::ElementId;

/// A symbolic reference to an exported symbol: `doc#name`, or `#name` for a lookup by name
/// alone. It is resolved at link time, against the symbol table.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolRef {
	/// The unit, as written (`config` in `config#pages`).
	pub unit: Option<String>,
	pub name: String,
}

impl SymbolRef {
	pub fn new(unit: Option<String>, name: impl Into<String>) -> Self {
		Self {
			unit,
			name: name.into(),
		}
	}
}

impl fmt::Display for SymbolRef {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}#{}", self.unit.as_deref().unwrap_or(""), self.name)
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SymbolKind {
	Heading,
	Figure,
	Anchor,
	/// An exported variable (`:export`).
	Variable,
}

impl SymbolKind {
	fn name(self) -> &'static str {
		match self {
			SymbolKind::Heading => "heading",
			SymbolKind::Figure => "figure",
			SymbolKind::Anchor => "anchor",
			SymbolKind::Variable => "variable",
		}
	}
}

/// A symbol a unit exports.
#[derive(Debug, Clone, PartialEq)]
pub struct Export {
	pub name: String,
	pub kind: SymbolKind,
	/// The element that declares it.
	pub element: Option<ElementId>,
	/// What dependents can see: target fragment, title, caption, number, value, … Sorted by
	/// name, so the fingerprint does not depend on the order they were added in.
	properties: Vec<(String, String)>,
}

impl Export {
	pub fn new(name: impl Into<String>, kind: SymbolKind, element: Option<ElementId>) -> Self {
		Self {
			name: name.into(),
			kind,
			element,
			properties: Vec::new(),
		}
	}

	/// Sets a property that dependents can see.
	pub fn with(mut self, property: impl Into<String>, value: impl Into<String>) -> Self {
		let property = property.into();
		let value = value.into();
		match self.properties.binary_search_by(|(p, _)| p.cmp(&property)) {
			Ok(index) => self.properties[index].1 = value,
			Err(index) => self.properties.insert(index, (property, value)),
		}
		self
	}

	pub fn property(&self, property: &str) -> Option<&str> {
		self.properties
			.iter()
			.find(|(p, _)| p == property)
			.map(|(_, value)| value.as_str())
	}

	pub fn properties(&self) -> &[(String, String)] {
		&self.properties
	}

	/// A hash of everything dependents can see. It is stable across runs and Rust versions,
	/// since it is stored in the workspace database. The element is not part of it: moving a
	/// heading does not change what links to it display.
	pub fn fingerprint(&self) -> u64 {
		let mut hash = Fnv::default();
		hash.field(self.kind.name());
		hash.field(&self.name);
		for (property, value) in &self.properties {
			hash.field(property);
			hash.field(value);
		}
		hash.0
	}
}

/// FNV-1a, 64 bits.
struct Fnv(u64);

impl Default for Fnv {
	fn default() -> Self {
		Self(0xcbf2_9ce4_8422_2325)
	}
}

impl Fnv {
	fn bytes(&mut self, bytes: &[u8]) {
		for byte in bytes {
			self.0 ^= u64::from(*byte);
			self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
		}
	}

	/// A field, prefixed with its length so that fields cannot run into each other.
	fn field(&mut self, text: &str) {
		self.bytes(&(text.len() as u64).to_le_bytes());
		self.bytes(text.as_bytes());
	}
}

/// A reference a unit makes, recorded symbolically.
#[derive(Debug, Clone, PartialEq)]
pub struct Use {
	pub target: SymbolRef,
	/// Where it is written.
	pub token: Token,
}

/// Something a unit's parse depends on: when it changes, the unit is parsed again.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Dependency {
	/// An imported module, by module path (`std.toc`). Its grammar and Lua are part of it.
	Import(String),
	/// An included file, by path relative to the workspace.
	Include(String),
	/// A variable exported by another unit.
	Variable(SymbolRef),
	/// The workspace configuration.
	Config,
	/// A workspace query run during the parse, with a fingerprint of its result.
	Query { query: String, fingerprint: u64 },
}

/// Everything a parse produces for a unit.
#[derive(Debug, Clone)]
pub struct Unit {
	pub document: Document,
	pub exports: Vec<Export>,
	pub uses: Vec<Use>,
	pub dependencies: Vec<Dependency>,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn symbol_refs() {
		assert_eq!(
			SymbolRef::new(Some("config".into()), "pages").to_string(),
			"config#pages"
		);
		assert_eq!(SymbolRef::new(None, "builds").to_string(), "#builds");
	}

	#[test]
	fn fingerprints() {
		let heading = |title: &str| {
			Export::new("builds", SymbolKind::Heading, Some(ElementId(3))).with("title", title)
		};
		let a = heading("Incremental builds").with("number", "2");
		let b = Export::new("builds", SymbolKind::Heading, Some(ElementId(9)))
			.with("number", "2")
			.with("title", "Incremental builds");
		assert_eq!(a.fingerprint(), b.fingerprint());
		assert_eq!(a.property("title"), Some("Incremental builds"));

		assert_ne!(
			a.fingerprint(),
			heading("Builds").with("number", "2").fingerprint()
		);
		let anchor = Export::new("builds", SymbolKind::Anchor, None)
			.with("number", "2")
			.with("title", "Incremental builds");
		assert_ne!(a.fingerprint(), anchor.fingerprint());
		// Length prefixes keep `ab` + `c` apart from `a` + `bc`.
		assert_ne!(
			Export::new("x", SymbolKind::Anchor, None)
				.with("ab", "c")
				.fingerprint(),
			Export::new("x", SymbolKind::Anchor, None)
				.with("a", "bc")
				.fingerprint()
		);
		// Pinned, since fingerprints are stored.
		assert_eq!(
			Export::new("x", SymbolKind::Anchor, None).fingerprint(),
			0xacd3_a1b6_3407_0ff9
		);
	}
}
