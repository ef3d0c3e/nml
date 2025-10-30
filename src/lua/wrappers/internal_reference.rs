use std::sync::Arc;

use graphviz_rust::print;
use mlua::UserData;
use mlua::Value;

use crate::add_documented_method;
use crate::add_documented_method_mut;
use crate::lua::kernel::Kernel;
use crate::lua::wrappers::InternalReferenceWrapper;
use crate::unit::references::InternalReference;
use crate::unit::references::Refname;

impl UserData for InternalReferenceWrapper {
	fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
	    fields.add_field_method_set("foo", |lua, this, value: mlua::Value| {
			Ok(())
		});
	}
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		add_documented_method!(
			methods,
			"Reference",
			"is_some",
			|_lua, this, ()| { Ok(this.0.is_some()) },
			"Return true if the reference is set",
			vec!["self",],
			Some("bool True if the reference is set")
		);
		add_documented_method!(
			methods,
			"Reference",
			"get",
			|lua, this, ()| {
				match &this.0 {
					Some(r) => {
						let r: &'static _ = unsafe { &*Arc::as_ptr(r) };
						Ok(Value::UserData(lua.create_userdata(r)?))
					}
					None => Ok(Value::Nil),
				}
			},
			"Get the reference value, or nil if unset",
			vec!["self",],
			Some("reference? The reference value")
		);
		add_documented_method_mut!(
			methods,
			"Reference",
			"set",
			|lua, this, (name,): (Option<Refname>,)| {
				let Some(name) = name else {
					this.0 = None;
					return Ok(());
				};
				let Refname::Internal(_) = &name else {
					return Err(mlua::Error::BadArgument {
						to: Some("reference:set()".into()),
						pos: 1,
						name: Some("name".into()),
						cause: Arc::new(mlua::Error::RuntimeError(
							"Expected an internal reference name".into(),
						)),
					});
				};
				Kernel::with_context(lua, |ctx| {
					this.0 = Some(Arc::new(InternalReference::new(ctx.location.clone(), name)));
				});
				Ok(())
			},
			"",
			vec!["self",],
			Some("bool true if the reference is set")
		);
	}
}
