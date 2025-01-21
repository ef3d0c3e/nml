use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use rusqlite::types::FromSql;
use rusqlite::Connection;
use rusqlite::ToSql;
use tokio::sync::Mutex;
use tokio::sync::MutexGuard;

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

	fn init<'con>(con: &MutexGuard<'con, Connection>) -> Result<(), rusqlite::Error> {
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
	fn cached<'con, E, F>(
		&self,
		con: &MutexGuard<'con, Connection>,
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
pub struct Cache
{
	con: Arc<Mutex<Connection>>,
}

impl Cache
{
	pub fn new(db_path: Option<&str>) -> Result<Self, String> {
		let con = db_path
				.map_or(Connection::open_in_memory(), Connection::open)
				.map_err(|err| format!("Unable to open connection to the database: {err}"))?;
		Ok(Self {
			con: Arc::new(Mutex::new(con))
		})
	}

	pub async fn get_connection<'s>(&'s self) -> MutexGuard<'s, Connection> {
		self.con.lock().await
	}
}
