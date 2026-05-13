use std::sync::Arc;

use mlua::{Lua, Table};

use crate::{
	add_documented_function,
	compiler::{compiler::Target, links::get_unique_link, sanitize::Sanitizer},
	lua::{kernel::Kernel, wrappers::UnitWrapper},
	parser::source::VirtualSource,
	unit::translation::{TranslationAccessors, TranslationUnit},
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

	nml.set(
		"with_translation_unit",
		lua.create_function(|lua, (name, f): (String, mlua::Function)| {
			Kernel::with_context(lua, |ctx|  {
				let source = Arc::new(VirtualSource::new(
					ctx.location.clone(),
					name.into(),
					String::default(),
				));

				let mut unit = TranslationUnit::new(
					ctx.unit.path.clone(),
					ctx.unit.parser.clone(),
					source,
					false,
					false,
				);
				unit.update_settings(ctx.unit.get_settings().clone());
				let wrapper = UnitWrapper(&mut unit);

				f.call::<mlua::Value>(wrapper)
			})
		})
		.unwrap(),
	)
	.unwrap();
}
