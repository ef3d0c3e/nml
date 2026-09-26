use std::fmt;
use std::ops::Range;

use nml_source::Token;

use crate::Document;
use crate::ElementId;

/// Where a reference looks for its symbol.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope {
	/// The current document: `[[name]]`.
	Local,
	/// The unit with this reference key: `[[doc#name]]`.
	Unit(String),
	/// Every unit; the symbol must be unique: `[[#name]]`.
	Any,
}

/// A symbolic reference, resolved at link time against the symbol table. With an empty
/// name in a unit's scope (`[[doc#]]`), it refers to the unit's page itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolRef {
	pub scope: Scope,
	pub name: String,
}

impl SymbolRef {
	pub fn local(name: impl Into<String>) -> Self {
		Self {
			scope: Scope::Local,
			name: name.into(),
		}
	}

	pub fn in_unit(unit: impl Into<String>, name: impl Into<String>) -> Self {
		Self {
			scope: Scope::Unit(unit.into()),
			name: name.into(),
		}
	}

	pub fn any(name: impl Into<String>) -> Self {
		Self {
			scope: Scope::Any,
			name: name.into(),
		}
	}

	/// The page of a unit.
	pub fn page(unit: impl Into<String>) -> Self {
		Self::in_unit(unit, "")
	}

	/// Whether it refers to a unit's page rather than a symbol.
	pub fn is_page(&self) -> bool {
		self.name.is_empty()
	}

	/// Parses `name`, `doc#name`, `doc#` or `#name`. Names and unit keys are not empty
	/// (except the name of a page reference), and hold no blanks, `#` or `|`.
	pub fn parse(text: &str) -> Option<Self> {
		let valid = |part: &str| {
			!part.is_empty()
				&& !part
					.chars()
					.any(|c| c.is_whitespace() || c == '#' || c == '|')
		};
		match text.split_once('#') {
			None => valid(text).then(|| Self::local(text)),
			Some(("", name)) => valid(name).then(|| Self::any(name)),
			Some((unit, "")) => valid(unit).then(|| Self::page(unit)),
			Some((unit, name)) => (valid(unit) && valid(name)).then(|| Self::in_unit(unit, name)),
		}
	}
}

impl fmt::Display for SymbolRef {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match &self.scope {
			Scope::Local => write!(f, "{}", self.name),
			Scope::Unit(unit) => write!(f, "{unit}#{}", self.name),
			Scope::Any => write!(f, "#{}", self.name),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SymbolKind {
	Heading,
	Figure,
	/// A medium with a name, outside of a figure or in one.
	Media,
	Table,
	/// A code block with a name: a numbered listing.
	Listing,
	Anchor,
	/// An exported variable (`:export`).
	Variable,
}

impl SymbolKind {
	pub const ALL: [SymbolKind; 7] = [
		SymbolKind::Heading,
		SymbolKind::Figure,
		SymbolKind::Media,
		SymbolKind::Table,
		SymbolKind::Listing,
		SymbolKind::Anchor,
		SymbolKind::Variable,
	];

	/// The name of the kind, as it is stored.
	pub fn name(self) -> &'static str {
		match self {
			SymbolKind::Heading => "heading",
			SymbolKind::Figure => "figure",
			SymbolKind::Media => "media",
			SymbolKind::Table => "table",
			SymbolKind::Listing => "listing",
			SymbolKind::Anchor => "anchor",
			SymbolKind::Variable => "variable",
		}
	}

	pub fn from_name(name: &str) -> Option<Self> {
		Self::ALL.into_iter().find(|kind| kind.name() == name)
	}
}

/// A symbol a unit exports.
#[derive(Debug, Clone, PartialEq)]
pub struct Export {
	pub name: String,
	pub kind: SymbolKind,
	/// The element that declares it.
	pub element: Option<ElementId>,
	/// Where it is declared: the workspace path of the file (the unit's, or an included
	/// one) and the range of the name. For editors; not part of the fingerprint.
	pub declared: Option<(String, Range<usize>)>,
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
			declared: None,
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
	/// A file read by a script, by workspace path, with the hash of its content when it was
	/// read.
	File { path: String, hash: String },
	/// A workspace query run during the parse, with a fingerprint of its result.
	Query { query: String, fingerprint: u64 },
	/// An environment variable read by a script, with the hash of its value when it was read
	/// (see `nml_lang::env_hash`).
	Env { name: String, hash: String },
	/// The clock, read by a script: the unit is parsed again on every build.
	Clock,
}

/// Everything a parse produces for a unit.
#[derive(Debug, Clone)]
pub struct Unit {
	/// The unit's reference key, if the document sets one (`nml.reference_key`). By
	/// default, it is the file's basename.
	pub reference_key: Option<String>,
	/// Where the unit's page is written in the output directory, if the document sets it
	/// (`nml.output`): the variable's value. By default, it is the file's path with `.html`.
	pub output: Option<Token>,
	pub document: Document,
	pub exports: Vec<Export>,
	pub uses: Vec<Use>,
	pub dependencies: Vec<Dependency>,
	/// Workspace files the unit's page shows (images, videos, …): they are copied to the
	/// output with it.
	pub assets: Vec<Asset>,
}

/// A workspace file a page shows, and where its copy goes in the output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
	/// Its workspace path.
	pub path: String,
	/// The path of its copy, relative to the output directory.
	pub output: String,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn symbol_refs() {
		for (text, expected) in [
			("builds", Some(SymbolRef::local("builds"))),
			("config#pages", Some(SymbolRef::in_unit("config", "pages"))),
			("config#", Some(SymbolRef::page("config"))),
			("#builds", Some(SymbolRef::any("builds"))),
			("", None),
			("#", None),
			("a#b#c", None),
			("a b", None),
			("a#b c", None),
			("a|b", None),
		] {
			let parsed = SymbolRef::parse(text);
			assert_eq!(parsed, expected, "parsing {text:?}");
			if let Some(parsed) = parsed {
				assert_eq!(parsed.to_string(), text);
			}
		}
		assert!(SymbolRef::page("config").is_page());
		assert!(!SymbolRef::local("x").is_page());
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
