use std::sync::Arc;

use mlua::{Lua, Table};

use crate::{
	compiler::{compiler::Target, links::get_unique_link, sanitize::Sanitizer},
	lua::kernel::Kernel,
};

fn parse_target(target: &str) -> mlua::Result<Target> {
	match target {
		"html" | "HTML" => Ok(Target::HTML),
		_ => Err(mlua::Error::BadArgument {
			to: Some("nml.escape".into()),
			pos: 1,
			name: Some("target".into()),
			cause: Arc::new(mlua::Error::RuntimeError(format!(
				"Invalid escape target `{target}'"
			))),
		}),
	}
}

pub fn kernel_utils(lua: &Lua, nml: &Table) {
	nml.set(
		"escape",
		lua.create_function(|_lua, (target, content): (String, String)| {
			let san = Sanitizer::new(parse_target(&target)?);
			Ok(san.sanitize(&content))
		})
		.unwrap(),
	)
	.unwrap();

	nml.set(
		"get_refname",
		lua.create_function(|lua, (target, name): (String, String)| {
			let target = parse_target(&target)?;
			Kernel::with_context(lua, |ctx| {
				let mut lock = ctx.unit.used_links.write();
				let inner = lock.as_mut().unwrap();
				Ok(get_unique_link(target, inner, &name))
			})
		})
		.unwrap(),
	)
	.unwrap();
}
