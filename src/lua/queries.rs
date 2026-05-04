use std::{ffi::OsStr, path::PathBuf};

use graphviz_rust::print;
use mlua::{Lua, Table};
use serde::Serialize;

use crate::{
	lua::kernel::Kernel,
	unit::{unit::DatabaseUnit, variable::VAR_TO_LUA},
};

/// Register functions for querying the cache
pub fn kernel_queries(lua: &Lua, nml: &Table) {
	let query = lua.create_table().unwrap();
	query
		.set(
			"units",
			lua.create_function(|lua, ()| {
				Kernel::with_context(lua, |ctx| {
					let Some(cache) = &ctx.kernel.cache else {
						return Ok(mlua::Value::Table(lua.create_table().unwrap()));
					};

					let con = tokio::runtime::Runtime::new()
						.unwrap()
						.block_on(cache.get_connection());
					let mut stmt = con.prepare("SELECT * FROM referenceable_units;").unwrap();
					let units = stmt
						.query_and_then([], |row| {
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

	query
		.set(
			"variable",
			lua.create_function(|lua, (unit_ref, name,): (String, String,)| {
				Kernel::with_context(lua, |ctx| {
					let Some(cache) = &ctx.kernel.cache else {
						return Ok(mlua::Value::Nil);
					};

					let con = tokio::runtime::Runtime::new()
						.unwrap()
						.block_on(cache.get_connection());
					let mut stmt = con.prepare("SELECT * FROM exported_variables WHERE name = (?1) AND unit_ref = (?2);").unwrap();
					Ok(stmt
						.query_row([name, unit_ref], |row| {
							let typename = row.get::<_, String>(4).unwrap();
							let data = row.get::<_, Vec<u8>>(5).unwrap();
							let cv = VAR_TO_LUA.get(&typename).expect("Invalid variable type");
							Ok(cv(lua, data))
						})
						.unwrap_or(mlua::Value::Nil))
				})
			})
			.unwrap(),
		)
		.unwrap();

	nml.set("query", query).unwrap();
}
