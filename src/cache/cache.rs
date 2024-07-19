use std::{error::Error, path::PathBuf};

use rusqlite::{types::FromSql, Connection, Params, ToSql};

struct Cache {
	con: Connection
}

impl Cache {
    fn new(file: PathBuf) -> Result<Self, String> {
		match Connection::open(file)
		{
			Err(e) => return Err(format!("Could not connect to cache database: {}", e.to_string())),
			Ok(con) => Ok(Self { con })
		}
    }
}

pub enum CachedError<E>
{
	SqlErr(rusqlite::Error),
	GenErr(E)
}

pub trait Cached
{
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

	fn init(con: &mut Connection) -> Result<(), rusqlite::Error>
	{
		con.execute(<Self as Cached>::sql_table(), ())
			.map(|_| ())
	}

	fn cached<E, F>(&self, con: &mut Connection, f: F)
		-> Result<<Self as Cached>::Value, CachedError<E>>
	where
		<Self as Cached>::Key: ToSql,
		<Self as Cached>::Value: FromSql + ToSql,
		F: FnOnce(&Self) -> Result<<Self as Cached>::Value, E>,
    {
		let key = self.key();

		// Find in cache
		let mut query = match con.prepare(<Self as Cached>::sql_get_query())
		{
			Ok(query) => query,
			Err(e) => return Err(CachedError::SqlErr(e))
		};

		let value = query.query_row([&key], |row|
		{
			Ok(row.get_unwrap::<_, <Self as Cached>::Value>(0))
		}).ok();

		if let Some(value) = value
		{
			// Found in cache
			return Ok(value)
		}
		else
		{
			// Compute a value
			let value = match f(&self)
			{
				Ok(val) => val,
				Err(e) => return Err(CachedError::GenErr(e))
			};

			// Try to insert
			let mut query = match con.prepare(<Self as Cached>::sql_insert_query())
			{
				Ok(query) => query,
				Err(e) => return Err(CachedError::SqlErr(e))
			};

			match query.execute((&key, &value))
			{
				Ok(_) => Ok(value),
				Err(e) => Err(CachedError::SqlErr(e))
			}
		}
    }
}
