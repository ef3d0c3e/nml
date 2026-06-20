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
use nml_document::Scope;
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
pub const SCHEMA_VERSION: i64 = 4;

const SCHEMA: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value ANY) STRICT;
CREATE TABLE units (
	id INTEGER PRIMARY KEY,
	path TEXT NOT NULL UNIQUE,
	name TEXT NOT NULL,
	title TEXT NOT NULL,
	role TEXT NOT NULL,
	mtime INTEGER NOT NULL,
	size INTEGER NOT NULL,
	hash BLOB NOT NULL,
	link_errors INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE INDEX units_by_name ON units(name);
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
	scope TEXT NOT NULL,
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
	/// The reference key: `doc` in `[[doc#name]]`.
	pub name: String,
	/// The title links to its page show.
	pub title: String,
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
	/// The reference key.
	pub name: &'a str,
	pub title: &'a str,
	pub role: Role,
	pub state: FileState,
	pub exports: &'a [Export],
	pub uses: &'a [StoredUse],
	pub dependencies: &'a [Dependency],
	/// Only pages have one.
	pub template: Option<&'a StoredTemplate>,
}

/// A symbol whose visible state changed, for the link stage.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolChange {
	/// The path of the exporting unit.
	pub unit: String,
	pub name: String,
	pub kind: SymbolChangeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SymbolChangeKind {
	Added,
	Removed,
	/// Its fingerprint changed: what links to it display may differ.
	Changed,
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
				"SELECT id, path, name, title, role, mtime, size, hash FROM units WHERE path = ?1",
				[path],
				unit_record,
			)
			.optional()?)
	}

	pub fn unit_by_id(&self, id: i64) -> Result<Option<UnitRecord>> {
		Ok(self
			.connection
			.query_row(
				"SELECT id, path, name, title, role, mtime, size, hash FROM units WHERE id = ?1",
				[id],
				unit_record,
			)
			.optional()?)
	}

	/// The units with this reference key: more than one makes references to it ambiguous.
	pub fn units_named(&self, name: &str) -> Result<Vec<UnitRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT id, path, name, title, role, mtime, size, hash FROM units
			WHERE name = ?1 ORDER BY path",
		)?;
		let units = statement
			.query_map([name], unit_record)?
			.collect::<rusqlite::Result<_>>()?;
		Ok(units)
	}

	/// The IDs of the exporting units of every symbol with this name.
	pub fn exporters(&self, name: &str) -> Result<Vec<i64>> {
		let mut statement = self
			.connection
			.prepare("SELECT unit FROM exports WHERE name = ?1 ORDER BY unit")?;
		let units = statement
			.query_map([name], |row| row.get(0))?
			.collect::<rusqlite::Result<_>>()?;
		Ok(units)
	}

	/// The IDs of the units with a use that may resolve to symbol `name` of the unit with key
	/// `unit`: `[[unit#name]]` or `[[#name]]`. With an empty name, the uses of the unit's
	/// page (`[[unit#]]`).
	pub fn users(&self, unit: &str, name: &str) -> Result<Vec<i64>> {
		let mut statement = self.connection.prepare(
			"SELECT DISTINCT unit FROM uses WHERE target_name = ?2
			AND ((scope = 'unit' AND target_unit = ?1) OR (scope = 'any' AND ?2 != ''))
			ORDER BY unit",
		)?;
		let units = statement
			.query_map([unit, name], |row| row.get(0))?
			.collect::<rusqlite::Result<_>>()?;
		Ok(units)
	}

	/// Every unit, by path.
	pub fn units(&self) -> Result<Vec<UnitRecord>> {
		let mut statement = self.connection.prepare(
			"SELECT id, path, name, title, role, mtime, size, hash FROM units ORDER BY path",
		)?;
		let units = statement
			.query_map([], unit_record)?
			.collect::<rusqlite::Result<_>>()?;
		Ok(units)
	}

	/// Replaces everything stored for a unit, in one transaction. Returns the unit's ID and
	/// the symbols whose visible state changed.
	pub fn commit(&mut self, unit: &StoredUnit) -> Result<(i64, Vec<SymbolChange>)> {
		let tx = self.connection.transaction()?;
		let old = fingerprints(&tx, unit.path)?;
		let id = commit(&tx, unit)?;
		tx.commit()?;
		let new = unit
			.exports
			.iter()
			.map(|export| (export.name.clone(), export.fingerprint()))
			.collect();
		Ok((id, diff(unit.path, old, new)))
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

	/// A value stored with the index, such as the hash of the configuration it was built
	/// with.
	pub fn meta(&self, key: &str) -> Result<Option<String>> {
		Ok(self
			.connection
			.query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
				row.get(0)
			})
			.optional()?)
	}

	pub fn set_meta(&mut self, key: &str, value: &str) -> Result<()> {
		self.connection.execute(
			"INSERT INTO meta (key, value) VALUES (?1, ?2)
			ON CONFLICT (key) DO UPDATE SET value = ?2",
			[key, value],
		)?;
		Ok(())
	}

	/// The paths of the units that have this parse-time dependency.
	pub fn dependents(&self, dependency: &Dependency) -> Result<Vec<String>> {
		let (kind, value) = encode_dependency(dependency);
		let mut statement = self.connection.prepare(
			"SELECT DISTINCT units.path FROM dependencies
			JOIN units ON units.id = dependencies.unit
			WHERE dependencies.kind = ?1 AND dependencies.value = ?2 ORDER BY units.path",
		)?;
		let paths = statement
			.query_map([kind, value.as_str()], |row| row.get(0))?
			.collect::<rusqlite::Result<_>>()?;
		Ok(paths)
	}

	/// Records whether the last link of a page had errors. Such pages are linked again on
	/// every build, so their errors are reported until they are fixed; the diagnostics
	/// themselves are not stored.
	pub fn set_link_errors(&mut self, path: &str, errors: bool) -> Result<()> {
		self.connection.execute(
			"UPDATE units SET link_errors = ?2 WHERE path = ?1",
			params![path, errors],
		)?;
		Ok(())
	}

	/// The paths of the pages whose last link had errors.
	pub fn units_with_link_errors(&self) -> Result<Vec<String>> {
		let mut statement = self
			.connection
			.prepare("SELECT path FROM units WHERE link_errors ORDER BY path")?;
		let paths = statement
			.query_map([], |row| row.get(0))?
			.collect::<rusqlite::Result<_>>()?;
		Ok(paths)
	}

	/// Removes a unit and everything stored for it. Returns its exports, as removed symbols.
	pub fn remove(&mut self, path: &str) -> Result<Vec<SymbolChange>> {
		let tx = self.connection.transaction()?;
		let old = fingerprints(&tx, path)?;
		tx.execute("DELETE FROM units WHERE path = ?1", [path])?;
		tx.commit()?;
		Ok(diff(path, old, Vec::new()))
	}

	pub fn export(&self, unit: i64, name: &str) -> Result<Option<Export>> {
		Ok(self
			.exports(unit)?
			.into_iter()
			.find(|export| export.name == name))
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
			"SELECT scope, target_unit, target_name, start, end FROM uses WHERE unit = ?1 ORDER BY rowid",
		)?;
		let rows = statement.query_map([unit], |row| {
			Ok((
				row.get::<_, String>(0)?,
				row.get::<_, Option<String>>(1)?,
				row.get::<_, String>(2)?,
				offset(row, 3)?..offset(row, 4)?,
			))
		})?;
		let mut uses = Vec::new();
		for row in rows {
			let (scope, target_unit, name, range) = row?;
			uses.push(StoredUse {
				target: SymbolRef {
					scope: decode_scope(&scope, target_unit)?,
					name,
				},
				range,
			});
		}
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
	let role: String = row.get(4)?;
	Ok(UnitRecord {
		id: row.get(0)?,
		path: row.get(1)?,
		name: row.get(2)?,
		title: row.get(3)?,
		role: if role == "page" {
			Role::Page
		} else {
			Role::Library
		},
		state: FileState {
			mtime: row.get(5)?,
			size: row.get::<_, i64>(6)? as u64,
			hash: row.get(7)?,
		},
	})
}

/// The exports of a unit, by name, with their fingerprints.
fn fingerprints(tx: &Transaction, path: &str) -> Result<Vec<(String, u64)>> {
	let mut statement = tx.prepare(
		"SELECT exports.name, exports.fingerprint FROM exports
		JOIN units ON units.id = exports.unit WHERE units.path = ?1",
	)?;
	let rows = statement
		.query_map([path], |row| {
			Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
		})?
		.collect::<rusqlite::Result<_>>()?;
	Ok(rows)
}

/// The symbol changes between two export lists of a unit, sorted by name.
fn diff(unit: &str, old: Vec<(String, u64)>, new: Vec<(String, u64)>) -> Vec<SymbolChange> {
	let change = |name: &str, kind| SymbolChange {
		unit: unit.to_string(),
		name: name.to_string(),
		kind,
	};
	let mut changes = Vec::new();
	for (name, fingerprint) in &new {
		match old.iter().find(|(old, _)| old == name) {
			None => changes.push(change(name, SymbolChangeKind::Added)),
			Some((_, old)) if old != fingerprint => {
				changes.push(change(name, SymbolChangeKind::Changed));
			}
			Some(_) => {}
		}
	}
	for (name, _) in &old {
		if !new.iter().any(|(new, _)| new == name) {
			changes.push(change(name, SymbolChangeKind::Removed));
		}
	}
	changes.sort();
	changes
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
		"INSERT INTO units (path, name, title, role, mtime, size, hash)
		VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
		params![
			unit.path,
			unit.name,
			unit.title,
			role,
			state.mtime,
			state.size as i64,
			state.hash
		],
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
		"INSERT INTO uses (unit, scope, target_unit, target_name, start, end)
		VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
	)?;
	for stored in unit.uses {
		let (scope, target_unit) = encode_scope(&stored.target.scope);
		statement.execute(params![
			id,
			scope,
			target_unit,
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

fn encode_scope(scope: &Scope) -> (&'static str, Option<&str>) {
	match scope {
		Scope::Local => ("local", None),
		Scope::Unit(unit) => ("unit", Some(unit)),
		Scope::Any => ("any", None),
	}
}

fn decode_scope(scope: &str, unit: Option<String>) -> Result<Scope> {
	Ok(match (scope, unit) {
		("local", None) => Scope::Local,
		("unit", Some(unit)) => Scope::Unit(unit),
		("any", None) => Scope::Any,
		_ => return Err(corrupt(format!("invalid scope `{scope}`"))),
	})
}

fn encode_symbol(symbol: &SymbolRef) -> Json {
	let (scope, unit) = encode_scope(&symbol.scope);
	serde_json::json!({ "scope": scope, "unit": unit, "name": symbol.name })
}

fn decode_symbol(json: &Json) -> Result<SymbolRef> {
	let text = |key| json.get(key).and_then(Json::as_str);
	let name = text("name").ok_or_else(|| corrupt("symbol without a name"))?;
	let scope = text("scope").ok_or_else(|| corrupt("symbol without a scope"))?;
	Ok(SymbolRef {
		scope: decode_scope(scope, text("unit").map(String::from))?,
		name: name.into(),
	})
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
						target: SymbolRef::in_unit("config", "pages"),
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
		let uses = vec![
			StoredUse {
				target: SymbolRef::any("builds"),
				range: 1..9,
			},
			StoredUse {
				target: SymbolRef::local("x"),
				range: 10..15,
			},
			StoredUse {
				target: SymbolRef::page("config"),
				range: 20..32,
			},
		];
		let dependencies = vec![
			Dependency::Import("std.toc".into()),
			Dependency::Include("parts/a.nml".into()),
			Dependency::Variable(SymbolRef::in_unit("config", "version")),
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
				name: "a",
				title: "A",
				role: Role::Page,
				state: state(hash),
				exports: &exports,
				uses: &uses,
				dependencies: &dependencies,
				template: Some(&template),
			})
			.unwrap()
			.0
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
		let uses: Vec<_> = index
			.uses(id)
			.unwrap()
			.iter()
			.map(|u| (u.target.clone(), u.range.clone()))
			.collect();
		assert_eq!(
			uses,
			[
				(SymbolRef::any("builds"), 1..9),
				(SymbolRef::local("x"), 10..15),
				(SymbolRef::page("config"), 20..32)
			]
		);
		assert_eq!(record.name, "a");
		assert_eq!(record.title, "A");
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

		let (library, _) = index
			.commit(&StoredUnit {
				path: "a.nml",
				name: "a",
				title: "A",
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

	fn commit_exports(index: &mut Index, path: &str, exports: &[Export]) -> Vec<SymbolChange> {
		index
			.commit(&StoredUnit {
				path,
				name: "a",
				title: "A",
				role: Role::Page,
				state: state(0),
				exports,
				uses: &[],
				dependencies: &[],
				template: None,
			})
			.unwrap()
			.1
	}

	fn changes(changes: &[SymbolChange]) -> Vec<(&str, &str, SymbolChangeKind)> {
		changes
			.iter()
			.map(|c| (c.unit.as_str(), c.name.as_str(), c.kind))
			.collect()
	}

	#[test]
	fn symbol_changes() {
		use SymbolChangeKind::*;

		let heading = |name: &str, title: &str| {
			Export::new(name, SymbolKind::Heading, Some(ElementId::from_index(1)))
				.with("title", title)
		};
		let mut index = Index::open_in_memory().unwrap();

		let first = commit_exports(&mut index, "a.nml", &[heading("x", "X"), heading("y", "Y")]);
		assert_eq!(
			changes(&first),
			[("a.nml", "x", Added), ("a.nml", "y", Added)]
		);

		// Same fingerprints: a moved heading (another element) is not a change.
		let moved = Export::new("x", SymbolKind::Heading, Some(ElementId::from_index(9)))
			.with("title", "X");
		let same = commit_exports(&mut index, "a.nml", &[heading("y", "Y"), moved]);
		assert!(same.is_empty());

		let next = commit_exports(
			&mut index,
			"a.nml",
			&[heading("x", "New"), heading("z", "Z")],
		);
		assert_eq!(
			changes(&next),
			[
				("a.nml", "x", Changed),
				("a.nml", "y", Removed),
				("a.nml", "z", Added)
			]
		);

		// Another unit's symbols are its own.
		let other = commit_exports(&mut index, "b.nml", &[heading("x", "New")]);
		assert_eq!(changes(&other), [("b.nml", "x", Added)]);

		let removed = index.remove("a.nml").unwrap();
		assert_eq!(
			changes(&removed),
			[("a.nml", "x", Removed), ("a.nml", "z", Removed)]
		);
		assert!(index.remove("a.nml").unwrap().is_empty());
	}

	#[test]
	fn lookups() {
		let mut index = Index::open_in_memory().unwrap();
		let heading = |name: &str| Export::new(name, SymbolKind::Heading, None);
		let mut add = |path: &str, name: &str, exports: &[Export], uses: &[SymbolRef]| {
			let uses: Vec<_> = uses
				.iter()
				.map(|target| StoredUse {
					target: target.clone(),
					range: 0..1,
				})
				.collect();
			index
				.commit(&StoredUnit {
					path,
					name,
					title: name,
					role: Role::Page,
					state: state(0),
					exports,
					uses: &uses,
					dependencies: &[],
					template: None,
				})
				.unwrap()
				.0
		};
		let a = add("a.nml", "a", &[heading("x"), heading("y")], &[]);
		let b = add(
			"docs/b.nml",
			"b",
			&[heading("x")],
			&[SymbolRef::in_unit("a", "x")],
		);
		let c = add(
			"c.nml",
			"b",
			&[],
			&[SymbolRef::any("x"), SymbolRef::page("a")],
		);
		let d = add(
			"d.nml",
			"d",
			&[],
			&[SymbolRef::local("x"), SymbolRef::in_unit("a", "y")],
		);

		assert_eq!(index.unit_by_id(b).unwrap().unwrap().path, "docs/b.nml");
		let named: Vec<_> = index
			.units_named("b")
			.unwrap()
			.into_iter()
			.map(|u| u.id)
			.collect();
		assert_eq!(named, [c, b]);
		assert_eq!(index.exporters("x").unwrap(), [a, b]);
		assert_eq!(index.export(a, "y").unwrap().unwrap().name, "y");
		assert!(index.export(a, "z").unwrap().is_none());

		// Local uses never point to another unit.
		assert_eq!(index.users("a", "x").unwrap(), [b, c]);
		assert_eq!(index.users("a", "y").unwrap(), [d]);
		assert_eq!(index.users("b", "x").unwrap(), [c]);
		assert_eq!(index.users("a", "").unwrap(), [c]);
		assert!(index.users("z", "").unwrap().is_empty());
	}

	#[test]
	fn link_errors() {
		let mut index = Index::open_in_memory().unwrap();
		commit(&mut index, "a.nml", 1);
		commit(&mut index, "b.nml", 1);
		assert!(index.units_with_link_errors().unwrap().is_empty());
		index.set_link_errors("b.nml", true).unwrap();
		assert_eq!(index.units_with_link_errors().unwrap(), ["b.nml"]);
		// A new commit starts without errors: the unit is linked anyway.
		commit(&mut index, "b.nml", 2);
		assert!(index.units_with_link_errors().unwrap().is_empty());
		index.set_link_errors("a.nml", true).unwrap();
		index.set_link_errors("a.nml", false).unwrap();
		assert!(index.units_with_link_errors().unwrap().is_empty());
	}

	#[test]
	fn meta_and_dependents() {
		let mut index = Index::open_in_memory().unwrap();
		assert_eq!(index.meta("config_hash").unwrap(), None);
		index.set_meta("config_hash", "a").unwrap();
		index.set_meta("config_hash", "b").unwrap();
		assert_eq!(index.meta("config_hash").unwrap().as_deref(), Some("b"));

		commit(&mut index, "x.nml", 1);
		commit(&mut index, "y.nml", 1);
		assert_eq!(
			index
				.dependents(&Dependency::Import("std.toc".into()))
				.unwrap(),
			["x.nml", "y.nml"]
		);
		assert_eq!(
			index
				.dependents(&Dependency::Variable(SymbolRef::in_unit(
					"config", "version"
				)))
				.unwrap(),
			["x.nml", "y.nml"]
		);
		assert!(
			index
				.dependents(&Dependency::Import("other".into()))
				.unwrap()
				.is_empty()
		);
	}
}
