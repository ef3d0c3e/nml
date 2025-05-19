use std::rc::Rc;
use std::sync::Arc;

use rusqlite::params;
use rusqlite::types::FromSql;
use rusqlite::Connection;
use rusqlite::ToSql;
use tokio::sync::Mutex;
use tokio::sync::MutexGuard;

use crate::compiler::compiler::Target;
use crate::unit::element::ReferenceableElement;
use crate::unit::translation::TranslationUnit;
use crate::unit::unit::DatabaseUnit;
use crate::unit::unit::Reference;

pub enum CachedError<E> {
	SqlErr(rusqlite::Error),
	GenErr(E),
}

pub trait Cached {
	type Key;
	type Value;

	/// SQL Query to create the cache table
	/// Note: You must use `IF NOT EXIST`
	fn sql_table() -> &'static str;

	/// SQL Get query
	fn sql_get_query() -> &'static str;

	/// SQL insert query
	fn sql_insert_query() -> &'static str;

	fn key(&self) -> <Self as Cached>::Key;

	fn init(con: &MutexGuard<'_, Connection>) -> Result<(), rusqlite::Error> {
		con.execute(<Self as Cached>::sql_table(), ()).map(|_| ())
	}

	/// Attempts to retrieve a cached element from the compilation database
	/// or create it (and insert it), if it doesn't exist
	///
	/// # Error
	///
	/// Will return an error if the database connection(s) fail,
	/// or if not cached, an error from the generator `f`
	///
	/// Note that on error, `f` may still have been called
	fn cached<E, F>(
		&self,
		con: &MutexGuard<'_, Connection>,
		f: F,
	) -> Result<<Self as Cached>::Value, CachedError<E>>
	where
		<Self as Cached>::Key: ToSql,
		<Self as Cached>::Value: FromSql + ToSql,
		F: FnOnce(&Self) -> Result<<Self as Cached>::Value, E>,
	{
		let key = self.key();

		// Find in cache
		let mut query = match con.prepare(<Self as Cached>::sql_get_query()) {
			Ok(query) => query,
			Err(e) => return Err(CachedError::SqlErr(e)),
		};

		let value = query
			.query_row([&key], |row| {
				Ok(row.get_unwrap::<_, <Self as Cached>::Value>(0))
			})
			.ok();

		if let Some(value) = value {
			// Found in cache
			Ok(value)
		} else {
			// Compute a value
			let value = match f(self) {
				Ok(val) => val,
				Err(e) => return Err(CachedError::GenErr(e)),
			};

			// Try to insert
			let mut query = match con.prepare(<Self as Cached>::sql_insert_query()) {
				Ok(query) => query,
				Err(e) => return Err(CachedError::SqlErr(e)),
			};

			match query.execute((&key, &value)) {
				Ok(_) => Ok(value),
				Err(e) => Err(CachedError::SqlErr(e)),
			}
		}
	}
}

/// Handles caching of [`Cached`] elements
#[derive(Clone)]
pub struct Cache {
	con: Arc<Mutex<Connection>>,
}

impl Cache {
	pub fn new(db_path: Option<&str>) -> Result<Self, String> {
		let con = db_path
			.map_or(Connection::open_in_memory(), Connection::open)
			.map_err(|err| format!("Unable to open connection to the database: {err}"))?;
		Ok(Self {
			con: Arc::new(Mutex::new(con)),
		})
	}

	pub async fn get_connection<'s>(&'s self) -> MutexGuard<'s, Connection> {
		self.con.lock().await
	}

	/// Sets up cache tables
	pub fn setup_tables(&self) {
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.get_connection());

		// Table containing all compiled units
		con.execute(
			"CREATE TABLE IF NOT EXISTS units(
			input_file		TEXT PRIMARY KEY,
			mtime			INTEGER NOT NULL
		);",
			(),
		)
		.unwrap();
		// Table containing all units that can be referenced
		con.execute(
			"CREATE TABLE IF NOT EXISTS referenceable_units(
				reference_key	TEXT PRIMARY KEY,
				input_file		TEXT NOT NULL,
				output_file		TEXT,
				FOREIGN KEY(input_file) REFERENCES units(input_file)
			);",
			(),
		)
		.unwrap();
		// Table containing all referenceable objects
		con.execute(
			"CREATE TABLE IF NOT EXISTS exported_references(
				name			TEXT PRIMARY KEY,
				unit_ref		TEXT NOT NULL,
				token_start		INTEGER NOT NULL,			
				token_end		INTEGER NOT NULL,			
				type			TEXT NOT NULL,
				data			TEXT NOT NULL,
				link			TEXT,
				FOREIGN KEY(unit_ref) REFERENCES referenceable_units(reference_key)
			);",
			(),
		)
		.unwrap();
	}

	/// Loads offloaded units from the database
	pub fn load_units<F, E>(&self, mut input_it: F) -> Result<(), E>
	where
		F: FnMut(DatabaseUnit) -> Result<(), E>,
	{
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.get_connection());

		// Load from database
		let mut cmd = con.prepare("SELECT * FROM referenceable_units").unwrap();
		let unlodaded_iter = cmd
			.query_map([], |row| {
				Ok((row.get(0).unwrap(), row.get(1).unwrap(), row.get(2).ok()))
			})
			.unwrap();

		// Insert
		for unloaded in unlodaded_iter {
			let unloaded: (String, String, Option<String>) = unloaded.unwrap();
			input_it(DatabaseUnit {
				reference_key: unloaded.0.clone(),
				input_file: unloaded.1.clone(),
				output_file: unloaded.2,
			})?;
		}
		Ok(())
	}

	/// Export units
	pub fn export_units<'a, I>(&self, it: I, time_now: u64)
	where
		I: Iterator<Item = &'a TranslationUnit<'a>>,
	{
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.get_connection());

		let mut stmt = con
			.prepare(
				"INSERT OR REPLACE
			INTO units (input_file, mtime)
			VALUES (?1, ?2);",
			)
			.unwrap();

		for unit in it {
			stmt.execute(params![unit.input_path(), time_now]).unwrap();
		}
	}

	/// Gets a unit's mtime
	pub fn get_mtime(&self, input_file: &String) -> Option<u64> {
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.get_connection());

		con.query_row(
			"SELECT mtime
		FROM units
		WHERE input_file = (?1)",
			[input_file],
			|row| Ok(row.get_unwrap::<_, u64>(0)),
		)
		.ok()
	}

	/// Export a referenceable unit
	pub fn export_ref_unit(&self, unit: &TranslationUnit, input: &String, output: &Option<String>) {
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.get_connection());

		con.execute(
			"INSERT OR REPLACE
		INTO referenceable_units
			(reference_key, input_file, output_file)
		VALUES
			(?1, ?2, ?3)",
			(unit.reference_key(), input, output),
		)
		.unwrap();
	}

	/// Query reference from the cache
	pub fn query_reference(&self, unit: &DatabaseUnit, name: &str) -> Option<Reference> {
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.get_connection());

		con.query_row(
			"SELECT name, token_start, token_end, type, link
		FROM exported_references
		WHERE name = (?1) AND unit_ref = (?2)",
			[name, &unit.reference_key],
			|row| {
				Ok(Reference {
					refname: row.get_unwrap(0),
					refkey: row.get_unwrap(3),
					source_unit: unit.input_file.clone(),
					token: row.get_unwrap(1)..row.get_unwrap(2),
					link: row.get_unwrap(4),
				})
			},
		)
		.ok()
	}

	pub fn export_references<'a, I>(&self, reference_key: &String, refs: I) -> Result<(), String>
	where
		I: Iterator<Item = (&'a String, &'a Rc<dyn ReferenceableElement>)>,
	{
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.get_connection());

		let mut stmt = con
			.prepare(
				"INSERT OR REPLACE
			INTO exported_references (name, unit_ref, token_start, token_end, type, data)
			VALUES (?1, ?2, ?3, ?4, ?5, ?6 );",
			)
			.unwrap();
		for (name, reference) in refs {
			// FIXME: Proper type-erased serialization for referneceables
			let serialized = "TODO";
			let range = reference.original_location().range;
			stmt.execute(params![
				name,
				reference_key,
				range.start,
				range.end,
				reference.refcount_key(),
				serialized
			])
			.map_err(|err| {
				format!(
					"Failed to insert reference ({name}, {serialized}, {0}): {err:#?}",
					reference_key
				)
			})?;
		}
		Ok(())
	}

	/// Gets the link of a reference
	pub fn get_reference_link(&self, reference: &Reference, target: Target) -> Option<String> {
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.get_connection());

		let unit = con
			.query_row(
				"SELECT output_file
			FROM referenceable_units
			WHERE reference_key = (?1)",
				[&reference.source_unit],
				|row| Ok(row.get_unwrap::<_, String>(0)),
			)
			.ok()?;
		let link = con
			.query_row(
				"SELECT link
			FROM exported_references
			WHERE
				name = (?1)
				unit_ref = (?2)",
				(&reference.refname, &reference.source_unit),
				|row| Ok(row.get_unwrap::<_, String>(0)),
			)
			.ok()?;

		match target {
			Target::HTML => Some(format!("{unit}#{link}")),
			_ => todo!(),
		}
	}
}
