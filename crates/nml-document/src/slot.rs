use crate::SymbolRef;

/// Identifies a slot in its [`Document`](crate::Document).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SlotId(pub(crate) u32);

/// A place in the output that is filled later than where it is written.
#[derive(Debug, Clone, PartialEq)]
pub enum Slot {
	Link(LinkSlot),
	PostParse(PostParseSlot),
	Deferred(DeferredSlot),
	/// A slot filled before the document was finished, such as a post-parse slot: its
	/// markup is part of the page.
	Markup(String),
}

/// Filled at link time from a symbol of the symbol table, possibly in a later build than the
/// one that parsed the unit.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkSlot {
	pub target: SymbolRef,
	pub property: Property,
	pub context: OutputContext,
}

/// What a link slot needs from its symbol.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Property {
	/// The URL of the symbol's target.
	Target,
	/// Its title or caption.
	Title,
	/// Its number (figures, numbered headings).
	Number,
	/// Another exported property, by name.
	Other(String),
}

/// Where a slot's value goes in the output, which decides how it is escaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputContext {
	Text,
	Attribute,
	/// Markup inserted as is: the output of post-parse and deferred slots.
	Markup,
}

/// Filled when the unit's parse ends, by a callback in the unit's Lua state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostParseSlot {
	/// The callback, as registered in the unit's Lua state.
	pub callback: u32,
}

/// Filled at link time by a module function, in a fresh Lua state with the query API. It is
/// stored, so it holds a function reference and serialisable arguments, never a closure.
#[derive(Debug, Clone, PartialEq)]
pub struct DeferredSlot {
	/// The module path (`std.nav`).
	pub module: String,
	/// The function exported by the module.
	pub function: String,
	pub args: SerialValue,
}

/// A value that can be stored with a deferred slot.
#[derive(Debug, Clone, PartialEq)]
pub enum SerialValue {
	Nil,
	Bool(bool),
	Integer(i64),
	Number(f64),
	String(String),
	List(Vec<SerialValue>),
	/// A table with string keys, in key order.
	Map(Vec<(String, SerialValue)>),
}
