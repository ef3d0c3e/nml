use std::fmt;
use std::ops::Range;
use std::path::Path;

use nml_document::DeferredSlot;
use nml_document::Dependency;
use nml_document::ElementId;
use nml_document::Export;
use nml_document::LinkSlot;
use nml_document::OutputContext;
use nml_document::Property;
use nml_document::SerialValue;
use nml_document::Slot;
use nml_document::SymbolKind;
use nml_document::SymbolRef;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::Transaction;
use rusqlite::params;
use serde_json::Value as Json;

/// The version of the schema below. A database with another version is rebuilt.
pub const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value ANY) STRICT;
CREATE TABLE units (
	id INTEGER PRIMARY KEY,
	path TEXT NOT NULL UNIQUE,
	role TEXT NOT NULL,
	mtime INTEGER NOT NULL,
	size INTEGER NOT NULL,
	hash BLOB NOT NULL
) STRICT;
CREATE TABLE exports (
	unit INTEGER NOT NULL REFERENCES units(id) ON DELETE CASCADE,
	name TEXT NOT NULL,
	kind TEXT NOT NULL,
	element INTEGER,
	properties TEXT NOT NULL,
	fingerprint INTEGER NOT NULL,
	PRIMARY KEY (unit, name)
) STRICT;
CREATE INDEX exports_by_name ON exports(name);
CREATE TABLE uses (
	unit INTEGER NOT NULL REFERENCES units(id) ON DELETE CASCADE,
	target_unit TEXT,
	target_name TEXT NOT NULL,
	start INTEGER NOT NULL,
	end INTEGER NOT NULL
) STRICT;
CREATE INDEX uses_by_unit ON uses(unit);
CREATE INDEX uses_by_name ON uses(target_name);
CREATE TABLE dependencies (
	unit INTEGER NOT NULL REFERENCES units(id) ON DELETE CASCADE,
	kind TEXT NOT NULL,
	value TEXT NOT NULL
) STRICT;
CREATE INDEX dependencies_by_unit ON dependencies(unit);
CREATE INDEX dependencies_by_value ON dependencies(kind, value);
CREATE TABLE templates (
	unit INTEGER PRIMARY KEY REFERENCES units(id) ON DELETE CASCADE,
	content TEXT NOT NULL
) STRICT;
CREATE TABLE slots (
	unit INTEGER NOT NULL REFERENCES units(id) ON DELETE CASCADE,
	position INTEGER NOT NULL,
	context TEXT NOT NULL,
	start INTEGER NOT NULL,
	end INTEGER NOT NULL,
	slot TEXT NOT NULL
) STRICT;
CREATE INDEX slots_by_unit ON slots(unit);
";

/// An error of the database. Since the database can always be rebuilt from the sources,
/// the usual answer is to delete it.
#[derive(Debug)]
pub struct IndexError(rusqlite::Error);

impl fmt::Display for IndexError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "workspace database: {}", self.0)
	}
}

impl std::error::Error for IndexError {}

impl From<rusqlite::Error> for IndexError {
	fn from(error: rusqlite::Error) -> Self {
		Self(error)
	}
}

pub type Result<T> = std::result::Result<T, IndexError>;

/// Whether a unit is a page (it gets an output) or a library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
	Page,
	Library,
}

/// The state of a source file when its unit was last committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileState {
	/// Modification time, in nanoseconds since the Unix epoch.
	pub mtime: i64,
	pub size: u64,
	pub hash: Vec<u8>,
}

/// A unit's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitRecord {
	pub id: i64,
	/// Relative to the workspace root, with `/` separators.
	pub path: String,
	pub role: Role,
	pub state: FileState,
}

/// A use, as stored: the source is the unit's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredUse {
	pub target: SymbolRef,
	pub range: Range<usize>,
}

/// A compiled page, as stored: its template and slots.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredTemplate {
	pub content: String,
	/// In order of position.
	pub slots: Vec<StoredSlot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StoredSlot {
	/// A byte offset in the template.
	pub position: usize,
	pub context: OutputContext,
	/// Where the slot is written in the unit's file, for diagnostics at relink time.
	pub range: Range<usize>,
	pub slot: Slot,
}

/// Everything committed for a unit: its last valid parse.
#[derive(Debug, Clone)]
pub struct StoredUnit<'a> {
	pub path: &'a str,
	pub role: Role,
	pub state: FileState,
	pub exports: &'a [Export],
	pub uses: &'a [StoredUse],
	pub dependencies: &'a [Dependency],
	/// Only pages have one.
	pub template: Option<&'a StoredTemplate>,
}

/// The workspace database.
#[derive(Debug)]
pub struct Index {
	connection: Connection,
}

impl Index {
	/// Opens or creates the database. A database with another schema version is emptied
	/// and rebuilt.
	pub fn open(path: &Path) -> Result<Self> {
		if let Some(parent) = path.parent() {
			// A failure shows up when opening.
			let _ = std::fs::create_dir_all(parent);
		}
		let connection = Connection::open(path)?;
		connection.pragma_update(None, "journal_mode", "WAL")?;
		Self::init(connection)
	}

	pub fn open_in_memory() -> Result<Self> {
		Self::init(Connection::open_in_memory()?)
	}

	fn init(connection: Connection) -> Result<Self> {
		connection.pragma_update(None, "foreign_keys", true)?;
		let mut index = Self { connection };
		if index.version()? != Some(SCHEMA_VERSION) {
			index.reset()?;
		}
		Ok(index)
	}

	fn version(&self) -> Result<Option<i64>> {
		let has_meta: bool = self.connection.query_row(
			"SELECT count(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
			[],
			|row| row.get(0),
		)?;
		if !has_meta {
			return Ok(None);
		}
		Ok(self
			.connection
			.query_row(
				"SELECT value FROM meta WHERE key = 'schema_version'",
				[],
				|row| row.get(0),
			)
			.optional()?)
	}

	/// Drops every table and creates the schema.
	fn reset(&mut self) -> Result<()> {
		// Foreign keys can only be switched outside a transaction.
		self.connection.pragma_update(None, "foreign_keys", false)?;
		let tx = self.connection.transaction()?;
		let tables: Vec<String> = tx
			.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?
			.query_map([], |row| row.get(0))?
			.collect::<rusqlite::Result<_>>()?;
		for table in tables {
			tx.execute_batch(&format!("DROP TABLE IF EXISTS \"{table}\""))?;
		}
		tx.execute_batch(SCHEMA)?;
		tx.execute(
			"INSERT INTO meta (key, value) VALUES ('schema_version', ?1)",
			[SCHEMA_VERSION],
		)?;
		tx.commit()?;
		self.connection.pragma_update(None, "foreign_keys", true)?;
		Ok(())
	}

	pub fn unit(&self, path: &str) -> Result<Option<UnitRecord>> {
		Ok(self
			.connection
			.query_row(
				"SELECT id, path, role, mtime, size, hash FROM units WHERE path = ?1",
				[path],
				unit_record,
			)
			.optional()?)
	}

	/// Every unit, by path.
	pub fn units(&self) -> Result<Vec<UnitRecord>> {
		let mut statement = self
			.connection
			.prepare("SELECT id, path, role, mtime, size, hash FROM units ORDER BY path")?;
		let units = statement
			.query_map([], unit_record)?
			.collect::<rusqlite::Result<_>>()?;
		Ok(units)
	}

	/// Replaces everything stored for a unit, in one transaction. Returns the unit's ID.
	pub fn commit(&mut self, unit: &StoredUnit) -> Result<i64> {
		let tx = self.connection.transaction()?;
		let id = commit(&tx, unit)?;
		tx.commit()?;
		Ok(id)
	}

	/// Records a new state for a file whose content did not change (e.g. it was touched), so
	/// that the next scan takes the fast path.
	pub fn update_state(&mut self, path: &str, state: &FileState) -> Result<()> {
		self.connection.execute(
			"UPDATE units SET mtime = ?2, size = ?3, hash = ?4 WHERE path = ?1",
			params![path, state.mtime, state.size as i64, state.hash],
		)?;
		Ok(())
	}

	/// Removes a unit and everything stored for it.
	pub fn remove(&mut self, path: &str) -> Result<()> {
		self.connection
			.execute("DELETE FROM units WHERE path = ?1", [path])?;
		Ok(())
	}

	pub fn exports(&self, unit: i64) -> Result<Vec<Export>> {
		let mut statement = self.connection.prepare(
			"SELECT name, kind, element, properties FROM exports WHERE unit = ?1 ORDER BY rowid",
		)?;
		let rows = statement.query_map([unit], |row| {
			Ok((
				row.get::<_, String>(0)?,
				row.get::<_, String>(1)?,
				row.get::<_, Option<u32>>(2)?,
				row.get::<_, String>(3)?,
			))
		})?;
		let mut exports = Vec::new();
		for row in rows {
			let (name, kind, element, properties) = row?;
			let mut export = Export::new(
				name,
				symbol_kind(&kind)?,
				element.map(ElementId::from_index),
			);
			for (property, value) in object(&properties)? {
				export = export.with(property, value.as_str().unwrap_or_default());
			}
			exports.push(export);
		}
		Ok(exports)
	}

	pub fn uses(&self, unit: i64) -> Result<Vec<StoredUse>> {
		let mut statement = self.connection.prepare(
			"SELECT target_unit, target_name, start, end FROM uses WHERE unit = ?1 ORDER BY rowid",
		)?;
		let uses = statement
			.query_map([unit], |row| {
				Ok(StoredUse {
					target: SymbolRef::new(row.get(0)?, row.get::<_, String>(1)?),
					range: offset(row, 2)?..offset(row, 3)?,
				})
			})?
			.collect::<rusqlite::Result<_>>()?;
		Ok(uses)
	}

	pub fn dependencies(&self, unit: i64) -> Result<Vec<Dependency>> {
		let mut statement = self
			.connection
			.prepare("SELECT kind, value FROM dependencies WHERE unit = ?1 ORDER BY rowid")?;
		let rows = statement.query_map([unit], |row| {
			Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
		})?;
		let mut dependencies = Vec::new();
		for row in rows {
			let (kind, value) = row?;
			dependencies.push(dependency(&kind, &value)?);
		}
		Ok(dependencies)
	}

	pub fn template(&self, unit: i64) -> Result<Option<StoredTemplate>> {
		let Some(content) = self
			.connection
			.query_row(
				"SELECT content FROM templates WHERE unit = ?1",
				[unit],
				|row| row.get(0),
			)
			.optional()?
		else {
			return Ok(None);
		};
		let mut statement = self.connection.prepare(
			"SELECT position, context, start, end, slot FROM slots WHERE unit = ?1 ORDER BY position, rowid",
		)?;
		let rows = statement.query_map([unit], |row| {
			Ok((
				offset(row, 0)?,
				row.get::<_, String>(1)?,
				offset(row, 2)?..offset(row, 3)?,
				row.get::<_, String>(4)?,
			))
		})?;
		let mut slots = Vec::new();
		for row in rows {
			let (position, context, range, slot) = row?;
			slots.push(StoredSlot {
				position,
				context: output_context(&context)?,
				range,
				slot: decode_slot(&slot)?,
			});
		}
		Ok(Some(StoredTemplate { content, slots }))
	}
}

/// A byte offset or position, stored as an integer.
fn offset(row: &rusqlite::Row, column: usize) -> rusqlite::Result<usize> {
	Ok(row.get::<_, i64>(column)? as usize)
}

fn unit_record(row: &rusqlite::Row) -> rusqlite::Result<UnitRecord> {
	let role: String = row.get(2)?;
	Ok(UnitRecord {
		id: row.get(0)?,
		path: row.get(1)?,
		role: if role == "page" {
			Role::Page
		} else {
			Role::Library
		},
		state: FileState {
			mtime: row.get(3)?,
			size: row.get::<_, i64>(4)? as u64,
			hash: row.get(5)?,
		},
	})
}

fn commit(tx: &Transaction, unit: &StoredUnit) -> Result<i64> {
	let role = match unit.role {
		Role::Page => "page",
		Role::Library => "library",
	};
	let state = &unit.state;
	// Deleting the row also deletes everything stored for it.
	tx.execute("DELETE FROM units WHERE path = ?1", [unit.path])?;
	tx.execute(
		"INSERT INTO units (path, role, mtime, size, hash) VALUES (?1, ?2, ?3, ?4, ?5)",
		params![unit.path, role, state.mtime, state.size as i64, state.hash],
	)?;
	let id = tx.last_insert_rowid();

	let mut statement = tx.prepare(
		"INSERT INTO exports (unit, name, kind, element, properties, fingerprint)
		VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
	)?;
	for export in unit.exports {
		let properties: serde_json::Map<String, Json> = export
			.properties()
			.iter()
			.map(|(property, value)| (property.clone(), Json::from(value.as_str())))
			.collect();
		statement.execute(params![
			id,
			export.name,
			symbol_kind_name(export.kind),
			export.element.map(ElementId::index),
			Json::Object(properties).to_string(),
			export.fingerprint() as i64,
		])?;
	}

	let mut statement = tx.prepare(
		"INSERT INTO uses (unit, target_unit, target_name, start, end) VALUES (?1, ?2, ?3, ?4, ?5)",
	)?;
	for stored in unit.uses {
		statement.execute(params![
			id,
			stored.target.unit,
			stored.target.name,
			stored.range.start as i64,
			stored.range.end as i64,
		])?;
	}

	let mut statement =
		tx.prepare("INSERT INTO dependencies (unit, kind, value) VALUES (?1, ?2, ?3)")?;
	for dependency in unit.dependencies {
		let (kind, value) = encode_dependency(dependency);
		statement.execute(params![id, kind, value])?;
	}

	if let Some(template) = unit.template {
		tx.execute(
			"INSERT INTO templates (unit, content) VALUES (?1, ?2)",
			params![id, template.content],
		)?;
		let mut statement = tx.prepare(
			"INSERT INTO slots (unit, position, context, start, end, slot)
			VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
		)?;
		for slot in &template.slots {
			statement.execute(params![
				id,
				slot.position as i64,
				output_context_name(slot.context),
				slot.range.start as i64,
				slot.range.end as i64,
				encode_slot(&slot.slot).to_string(),
			])?;
		}
	}
	Ok(id)
}

/// An error for data that this schema version cannot have written.
fn corrupt(what: impl fmt::Display) -> IndexError {
	IndexError(rusqlite::Error::InvalidColumnType(
		0,
		what.to_string(),
		rusqlite::types::Type::Text,
	))
}

fn object(text: &str) -> Result<serde_json::Map<String, Json>> {
	match serde_json::from_str(text) {
		Ok(Json::Object(map)) => Ok(map),
		_ => Err(corrupt(format!("invalid JSON object `{text}`"))),
	}
}

fn symbol_kind_name(kind: SymbolKind) -> &'static str {
	match kind {
		SymbolKind::Heading => "heading",
		SymbolKind::Figure => "figure",
		SymbolKind::Anchor => "anchor",
		SymbolKind::Variable => "variable",
		_ => unreachable!("symbol kinds are listed here"),
	}
}

fn symbol_kind(name: &str) -> Result<SymbolKind> {
	Ok(match name {
		"heading" => SymbolKind::Heading,
		"figure" => SymbolKind::Figure,
		"anchor" => SymbolKind::Anchor,
		"variable" => SymbolKind::Variable,
		_ => return Err(corrupt(format!("unknown symbol kind `{name}`"))),
	})
}

fn output_context_name(context: OutputContext) -> &'static str {
	match context {
		OutputContext::Text => "text",
		OutputContext::Attribute => "attribute",
		OutputContext::Markup => "markup",
	}
}

fn output_context(name: &str) -> Result<OutputContext> {
	Ok(match name {
		"text" => OutputContext::Text,
		"attribute" => OutputContext::Attribute,
		"markup" => OutputContext::Markup,
		_ => return Err(corrupt(format!("unknown output context `{name}`"))),
	})
}

fn encode_dependency(dependency: &Dependency) -> (&'static str, String) {
	match dependency {
		Dependency::Import(module) => ("import", module.clone()),
		Dependency::Include(path) => ("include", path.clone()),
		Dependency::Variable(symbol) => ("variable", encode_symbol(symbol).to_string()),
		Dependency::Config => ("config", String::new()),
		Dependency::Query { query, fingerprint } => (
			"query",
			serde_json::json!({ "query": query, "fingerprint": *fingerprint as i64 }).to_string(),
		),
		_ => unreachable!("dependency kinds are listed here"),
	}
}

fn dependency(kind: &str, value: &str) -> Result<Dependency> {
	Ok(match kind {
		"import" => Dependency::Import(value.into()),
		"include" => Dependency::Include(value.into()),
		"variable" => Dependency::Variable(decode_symbol(&parse(value)?)?),
		"config" => Dependency::Config,
		"query" => {
			let map = object(value)?;
			Dependency::Query {
				query: map
					.get("query")
					.and_then(Json::as_str)
					.ok_or_else(|| corrupt("query without text"))?
					.into(),
				fingerprint: map
					.get("fingerprint")
					.and_then(Json::as_i64)
					.ok_or_else(|| corrupt("query without fingerprint"))?
					as u64,
			}
		}
		_ => return Err(corrupt(format!("unknown dependency kind `{kind}`"))),
	})
}

fn parse(text: &str) -> Result<Json> {
	serde_json::from_str(text).map_err(|_| corrupt(format!("invalid JSON `{text}`")))
}

fn encode_symbol(symbol: &SymbolRef) -> Json {
	serde_json::json!({ "unit": symbol.unit, "name": symbol.name })
}

fn decode_symbol(json: &Json) -> Result<SymbolRef> {
	let name = json
		.get("name")
		.and_then(Json::as_str)
		.ok_or_else(|| corrupt("symbol without a name"))?;
	let unit = json.get("unit").and_then(Json::as_str).map(String::from);
	Ok(SymbolRef::new(unit, name))
}

fn encode_slot(slot: &Slot) -> Json {
	match slot {
		Slot::Link(link) => {
			let property = match &link.property {
				Property::Target => Json::from("target"),
				Property::Title => Json::from("title"),
				Property::Number => Json::from("number"),
				Property::Other(name) => serde_json::json!({ "other": name }),
				_ => unreachable!("properties are listed here"),
			};
			serde_json::json!({
				"link": {
					"target": encode_symbol(&link.target),
					"property": property,
					"context": output_context_name(link.context),
				}
			})
		}
		Slot::Deferred(deferred) => serde_json::json!({
			"deferred": {
				"module": deferred.module,
				"function": deferred.function,
				"args": encode_value(&deferred.args),
			}
		}),
		// Post-parse slots are filled before the unit is committed.
		Slot::PostParse(_) => unreachable!("post-parse slots are never stored"),
	}
}

fn decode_slot(text: &str) -> Result<Slot> {
	let json = parse(text)?;
	if let Some(link) = json.get("link") {
		let property = match link.get("property") {
			Some(Json::String(name)) => match name.as_str() {
				"target" => Property::Target,
				"title" => Property::Title,
				"number" => Property::Number,
				_ => return Err(corrupt(format!("unknown property `{name}`"))),
			},
			Some(other) => Property::Other(
				other
					.get("other")
					.and_then(Json::as_str)
					.ok_or_else(|| corrupt("invalid property"))?
					.into(),
			),
			None => return Err(corrupt("link slot without a property")),
		};
		let context = link
			.get("context")
			.and_then(Json::as_str)
			.ok_or_else(|| corrupt("link slot without a context"))?;
		let target = link
			.get("target")
			.ok_or_else(|| corrupt("link slot without a target"))?;
		return Ok(Slot::Link(LinkSlot {
			target: decode_symbol(target)?,
			property,
			context: output_context(context)?,
		}));
	}
	if let Some(deferred) = json.get("deferred") {
		let text = |key: &str| {
			deferred
				.get(key)
				.and_then(Json::as_str)
				.map(String::from)
				.ok_or_else(|| corrupt(format!("deferred slot without `{key}`")))
		};
		return Ok(Slot::Deferred(DeferredSlot {
			module: text("module")?,
			function: text("function")?,
			args: decode_value(deferred.get("args").unwrap_or(&Json::Null))?,
		}));
	}
	Err(corrupt(format!("unknown slot `{text}`")))
}

/// Serial values in JSON. Numbers are tagged, so integers and floats survive the round trip.
fn encode_value(value: &SerialValue) -> Json {
	match value {
		SerialValue::Nil => Json::Null,
		SerialValue::Bool(b) => Json::from(*b),
		SerialValue::Integer(n) => serde_json::json!({ "int": n }),
		SerialValue::Number(n) => serde_json::json!({ "num": n }),
		SerialValue::String(s) => Json::from(s.as_str()),
		SerialValue::List(list) => Json::Array(list.iter().map(encode_value).collect()),
		SerialValue::Map(map) => serde_json::json!({
			"map": map.iter().map(|(k, v)| serde_json::json!([k, encode_value(v)])).collect::<Vec<_>>()
		}),
	}
}

fn decode_value(json: &Json) -> Result<SerialValue> {
	Ok(match json {
		Json::Null => SerialValue::Nil,
		Json::Bool(b) => SerialValue::Bool(*b),
		Json::String(s) => SerialValue::String(s.clone()),
		Json::Array(list) => {
			SerialValue::List(list.iter().map(decode_value).collect::<Result<_>>()?)
		}
		Json::Object(map) => {
			if let Some(n) = map.get("int").and_then(Json::as_i64) {
				SerialValue::Integer(n)
			} else if let Some(n) = map.get("num").and_then(Json::as_f64) {
				SerialValue::Number(n)
			} else if let Some(Json::Array(entries)) = map.get("map") {
				let mut decoded = Vec::new();
				for entry in entries {
					match entry.as_array().map(Vec::as_slice) {
						Some([Json::String(key), value]) => {
							decoded.push((key.clone(), decode_value(value)?));
						}
						_ => return Err(corrupt("invalid map entry")),
					}
				}
				SerialValue::Map(decoded)
			} else {
				return Err(corrupt("invalid serial value"));
			}
		}
		Json::Number(_) => return Err(corrupt("untagged number")),
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn state(hash: u8) -> FileState {
		FileState {
			mtime: 1_700_000_000_000_000_000,
			size: 42,
			hash: vec![hash; 32],
		}
	}

	fn exports() -> Vec<Export> {
		vec![
			Export::new(
				"builds",
				SymbolKind::Heading,
				Some(ElementId::from_index(3)),
			)
			.with("title", "Incremental builds"),
			Export::new("x", SymbolKind::Variable, None).with("value", "1"),
		]
	}

	fn template() -> StoredTemplate {
		StoredTemplate {
			content: "<a href=\"\"></a>".into(),
			slots: vec![
				StoredSlot {
					position: 9,
					context: OutputContext::Attribute,
					range: 4..20,
					slot: Slot::Link(LinkSlot {
						target: SymbolRef::new(Some("config".into()), "pages"),
						property: Property::Target,
						context: OutputContext::Attribute,
					}),
				},
				StoredSlot {
					position: 11,
					context: OutputContext::Markup,
					range: 30..40,
					slot: Slot::Deferred(DeferredSlot {
						module: "std.nav".into(),
						function: "menu".into(),
						args: SerialValue::Map(vec![
							("depth".into(), SerialValue::Integer(2)),
							("scale".into(), SerialValue::Number(1.5)),
							(
								"tags".into(),
								SerialValue::List(vec![
									SerialValue::String("a".into()),
									SerialValue::Bool(true),
									SerialValue::Nil,
								]),
							),
						]),
					}),
				},
			],
		}
	}

	fn commit(index: &mut Index, path: &str, hash: u8) -> i64 {
		let exports = exports();
		let uses = vec![StoredUse {
			target: SymbolRef::new(None, "builds"),
			range: 1..9,
		}];
		let dependencies = vec![
			Dependency::Import("std.toc".into()),
			Dependency::Include("parts/a.nml".into()),
			Dependency::Variable(SymbolRef::new(Some("config".into()), "version")),
			Dependency::Config,
			Dependency::Query {
				query: "pages".into(),
				fingerprint: u64::MAX - 1,
			},
		];
		let template = template();
		index
			.commit(&StoredUnit {
				path,
				role: Role::Page,
				state: state(hash),
				exports: &exports,
				uses: &uses,
				dependencies: &dependencies,
				template: Some(&template),
			})
			.unwrap()
	}

	#[test]
	fn round_trip() {
		let mut index = Index::open_in_memory().unwrap();
		let id = commit(&mut index, "docs/a.nml", 1);

		let record = index.unit("docs/a.nml").unwrap().unwrap();
		assert_eq!(record.id, id);
		assert_eq!(record.role, Role::Page);
		assert_eq!(record.state, state(1));

		let stored = index.exports(id).unwrap();
		assert_eq!(stored, exports());
		assert_eq!(stored[0].fingerprint(), exports()[0].fingerprint());
		assert_eq!(index.uses(id).unwrap()[0].target.to_string(), "#builds");
		let dependencies = index.dependencies(id).unwrap();
		assert_eq!(dependencies.len(), 5);
		assert_eq!(
			dependencies[4],
			Dependency::Query {
				query: "pages".into(),
				fingerprint: u64::MAX - 1
			}
		);
		assert_eq!(index.template(id).unwrap(), Some(template()));
	}

	#[test]
	fn commit_replaces_and_remove_deletes() {
		let mut index = Index::open_in_memory().unwrap();
		commit(&mut index, "a.nml", 1);
		commit(&mut index, "b.nml", 1);
		let id = commit(&mut index, "a.nml", 2);

		let units = index.units().unwrap();
		assert_eq!(units.len(), 2);
		assert_eq!(units[0].state.hash, vec![2; 32]);
		assert_eq!(index.exports(id).unwrap().len(), 2);

		let library = index
			.commit(&StoredUnit {
				path: "a.nml",
				role: Role::Library,
				state: state(3),
				exports: &[],
				uses: &[],
				dependencies: &[],
				template: None,
			})
			.unwrap();
		assert!(index.exports(library).unwrap().is_empty());
		assert_eq!(index.template(library).unwrap(), None);
		assert_eq!(index.unit("a.nml").unwrap().unwrap().role, Role::Library);

		index.remove("a.nml").unwrap();
		assert!(index.unit("a.nml").unwrap().is_none());
		let count: i64 = index
			.connection
			.query_row("SELECT count(*) FROM exports", [], |row| row.get(0))
			.unwrap();
		assert_eq!(count, 2, "only b.nml's exports are left");
	}

	#[test]
	fn other_schema_versions_are_rebuilt() {
		let dir = std::env::temp_dir().join(format!("nml-index-test-{}", std::process::id()));
		let path = dir.join("index.db");
		let _ = std::fs::remove_dir_all(&dir);
		{
			let mut index = Index::open(&path).unwrap();
			commit(&mut index, "a.nml", 1);
			index
				.connection
				.execute("UPDATE meta SET value = 0 WHERE key = 'schema_version'", [])
				.unwrap();
		}
		let index = Index::open(&path).unwrap();
		assert!(index.units().unwrap().is_empty());
		assert_eq!(index.version().unwrap(), Some(SCHEMA_VERSION));
		let mode: String = index
			.connection
			.query_row("PRAGMA journal_mode", [], |row| row.get(0))
			.unwrap();
		assert_eq!(mode, "wal");
		drop(index);
		// Reopening a current database keeps its content.
		let mut index = Index::open(&path).unwrap();
		commit(&mut index, "a.nml", 1);
		drop(index);
		assert_eq!(Index::open(&path).unwrap().units().unwrap().len(), 1);
		std::fs::remove_dir_all(dir).unwrap();
	}
}
