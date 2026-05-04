use std::{ffi::OsStr, path::PathBuf};

use graphviz_rust::print;
use mlua::{Lua, Table};
use serde::Serialize;

use crate::{lua::kernel::Kernel, unit::unit::DatabaseUnit};

/// Register functions for querying the cache
pub fn kernel_queries(lua: &Lua, nml: &Table) {
	let query = lua.create_table().unwrap();
	query
		.set(
			"units",
			lua.create_function(|lua, ()| {
				Kernel::with_context(lua, |ctx| {
					let Some(cache) = &ctx.kernel.cache else {
						println!("kern = ");
						return Ok(mlua::Value::Table(lua.create_table().unwrap()));
					};
					println!("HERE\n");

					let con = tokio::runtime::Runtime::new()
						.unwrap()
						.block_on(cache.get_connection());
					let mut stmt = con.prepare("SELECT * FROM referenceable_units;").unwrap();
					let units = stmt
						.query_and_then([], |row| {
							println!("queried={:#?}", row.get::<_, String>(0).unwrap());
							Result::<_, rusqlite::Error>::Ok(DatabaseUnit {
								reference_key: row.get::<_, String>(0).unwrap(),
								input_file: PathBuf::from(unsafe {
									OsStr::from_encoded_bytes_unchecked(
										&row.get::<_, Vec<u8>>(1).unwrap(),
									)
								}),
								output_file: row.get::<_, Option<Vec<u8>>>(2).unwrap().map(|buf| {
									PathBuf::from(unsafe {
										OsStr::from_encoded_bytes_unchecked(&buf)
									})
								}),
							})
						})
						.unwrap()
						.map(|res| res.unwrap())
						.collect::<Vec<_>>();
					Ok(mlua::LuaSerdeExt::to_value(lua, &units).unwrap())
				})
			})
			.unwrap(),
		)
		.unwrap();
	nml.set("query", query).unwrap();
}
