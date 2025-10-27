use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use rusqlite::params;
use rusqlite::types::FromSql;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::ToSql;
use tokio::sync::Mutex;
use tokio::sync::MutexGuard;

use crate::lsp::reference::LsReference;
use crate::parser::resolver::UnitDependency;
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
	pub fn new(db_path: &Path) -> Result<Self, String> {
		let con = if db_path.as_os_str().is_empty() {
			Connection::open_in_memory()
		} else {
			Connection::open(db_path)
		}
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
				FOREIGN KEY(unit_ref) REFERENCES referenceable_units(reference_key) ON DELETE CASCADE,
				UNIQUE(unit_ref, name)
			);",
			(),
		)
		.unwrap();
		// Table containing unit dependencies
		con.execute(
			"CREATE TABLE IF NOT EXISTS dependencies(
				unit_ref		TEXT NOT NULL,
				depends_on		TEXT NOT NULL,
				range_start    	INTEGER NOT NULL,
				range_end    	INTEGER NOT NULL,
				depends_for		TEXT NOT NULL,
				PRIMARY KEY(depends_for, unit_ref),
				FOREIGN KEY(unit_ref) REFERENCES referenceable_units(reference_key) ON DELETE CASCADE
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
			let unloaded: (String, Vec<u8>, Option<Vec<u8>>) = unloaded.unwrap();
			input_it(DatabaseUnit {
				reference_key: unloaded.0.clone(),
				input_file: PathBuf::from(unsafe { OsStr::from_encoded_bytes_unchecked(&unloaded.1) }),
				output_file: unloaded.2.map(|path| PathBuf::from(unsafe { OsStr::from_encoded_bytes_unchecked(&path) })),
			})?;
		}
		Ok(())
	}

	/// Export units
	pub fn export_units<'a, I>(&self, it: I, time_now: u64)
	where
		I: Iterator<Item = &'a TranslationUnit>,
	{
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.get_connection());

		let mut insert_stmt = con
			.prepare(
				"INSERT OR REPLACE
			INTO units (input_file, mtime)
			VALUES (?1, ?2);",
			)
			.unwrap();

		for unit in it {
			insert_stmt
				.execute(params![unit.input_path().as_os_str().as_bytes(), time_now])
				.unwrap();
		}
	}

	/// Gets a unit's mtime
	pub fn get_mtime(&self, input_file: &std::path::Path) -> Option<u64> {
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.get_connection());

		con.query_row(
			"SELECT mtime
		FROM units
		WHERE input_file = (?1)",
			[input_file.as_os_str().as_bytes()],
			|row| Ok(row.get_unwrap::<_, u64>(0)),
		)
		.ok()
	}

	/// Export a referenceable unit
	pub fn export_ref_unit(&self, unit: &TranslationUnit, input: &PathBuf, output: &Option<PathBuf>) {
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.get_connection());

		// Find if unit reference key changed
		if let Some(previous) = 
			con.query_row(
		"SELECT reference_key
		FROM referenceable_units
		WHERE input_file = (?1)",
		[input.as_os_str().as_bytes()], |row| {
			Ok(row.get_unwrap::<_, String>(0))
		}).optional().unwrap() {
			con.execute(
				"DELETE
				FROM referenceable_units
				WHERE reference_key = (?1);",
				[previous],
			).unwrap();
		}

		// Delete previous unit-related data
		con.execute(
			"DELETE
			FROM referenceable_units
			WHERE reference_key = (?1);",
			[unit.reference_key()],
		)
		.unwrap();
		// Insert new unit
		con.execute(
			"INSERT OR REPLACE
		INTO referenceable_units
			(reference_key, input_file, output_file)
		VALUES
			(?1, ?2, ?3)",
			(unit.reference_key(), input.as_os_str().as_bytes(), output.as_ref().map(|out| out.as_os_str().as_bytes())),
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

	/// Gets all exported references in the project
	pub async fn get_references(&self) -> Vec<LsReference> {
		let con = self.get_connection().await;

		let mut stmt = con
			.prepare(
				"SELECT
			name, unit_ref, token_start, token_end, type, ru.input_file
		FROM exported_references
		LEFT JOIN referenceable_units ru ON unit_ref = ru.reference_key;",
			)
			.unwrap();
		let res = stmt
			.query_map((), |row| {
				Ok(LsReference {
					name: row.get_unwrap::<_, String>(0),
					range: row.get_unwrap::<_, usize>(2)..row.get_unwrap::<_, usize>(3),
					source_path: PathBuf::from(unsafe{ OsStr::from_encoded_bytes_unchecked(&row.get_unwrap::<_, Vec<u8>>(5)) }),
					source_refkey: row.get_unwrap::<_, String>(1),
					reftype: row.get_unwrap::<_, String>(4),
				})
			})
			.unwrap();

		let mut result = vec![];
		for r in res {
			let Ok(reference) = r else { continue };
			result.push(reference);
		}
		result
	}

	pub fn export_references<'a, I>(&self, reference_key: &String, refs: I) -> Result<(), String>
	where
		I: Iterator<Item = (&'a String, &'a Arc<dyn ReferenceableElement>)>,
	{
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.get_connection());

		let mut stmt = con
			.prepare(
				"INSERT OR REPLACE
			INTO exported_references (name, unit_ref, token_start, token_end, type, data, link)
			VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
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
				serialized,
				reference.get_link().unwrap().clone(),
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

	/// Export dependencies to the cache, returns units that are missing dependencies
	pub fn export_dependencies(
		&self,
		deps: &HashMap<String, HashMap<String, Vec<UnitDependency>>>,
	) -> HashMap<String, Vec<UnitDependency>> {
		let mut con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.get_connection());

		let tx = con.transaction().unwrap();
		let delete_stmt = tx
			.prepare(
				"DELETE
			FROM dependencies
			WHERE unit_ref = (?1);",
			)
			.unwrap();
		let mut export_stmt = tx
			.prepare(
				"INSERT OR REPLACE
			INTO dependencies (unit_ref, depends_on, range_start, range_end, depends_for)
			VALUES (?1, ?2, ?3, ?4, ?5);",
			)
			.unwrap();
		// Export new dependencies
		for (unit_ref, map) in deps {
			//delete_stmt.execute([unit_ref]).unwrap();
			for (depends_on, list) in map {
				for dep in list {
					export_stmt
						.execute(params!(
							unit_ref,
							depends_on,
							dep.range.start,
							dep.range.end,
							dep.depends_for
						))
						.unwrap();
				}
			}
		}
		drop(export_stmt);
		drop(delete_stmt);
		tx.commit().unwrap();

		// Populate missing
		let mut update = con
			.prepare(
				"SELECT DISTINCT ru.input_file, dep.range_start, dep.range_end, dep.depends_for
		FROM dependencies AS dep
		JOIN referenceable_units AS ru
			ON ru.reference_key = dep.unit_ref
		WHERE NOT EXISTS (
			SELECT 1
			FROM exported_references AS ref
			WHERE ref.unit_ref = dep.depends_on
			AND ref.name = dep.depends_for
		);",
			)
			.unwrap();
		let mut missing: HashMap<String, Vec<UnitDependency>> = HashMap::new();
		update
			.query_map([], |row| {
				Ok((
					row.get_unwrap::<_, String>(0),
					row.get_unwrap::<_, usize>(1),
					row.get_unwrap::<_, usize>(2),
					row.get_unwrap::<_, String>(3),
				))
			})
			.unwrap()
			.into_iter()
			.map(|v| {
				let Ok((unit_ref, start, end, depends_for)) = v else {
					panic!()
				};
				if let Some(list) = missing.get_mut(&unit_ref) {
					list.push(UnitDependency {
						depends_for,
						range: start..end,
					});
				} else {
					missing.insert(
						unit_ref,
						vec![UnitDependency {
							depends_for,
							range: start..end,
						}],
					);
				}
			})
			.count();
		missing
	}
}
