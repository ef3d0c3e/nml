use mlua::UserData;
use mlua::Value;

use crate::lua::wrappers::ElemWrapper;
use crate::lua::wrappers::IteratorWrapper;
use crate::lua::wrappers::ScopeWrapper;

impl UserData for IteratorWrapper {
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		methods.add_meta_function(
			mlua::MetaMethod::Call,
			|lua, (this,): (mlua::AnyUserData,)| {
				let mut iter = this.borrow_mut::<IteratorWrapper>()?;
				if let Some((scope, elem)) = iter.0.next() {
					Ok(mlua::MultiValue::from_vec(vec![
						Value::UserData(lua.create_userdata(ScopeWrapper(scope)).unwrap()),
						Value::UserData(lua.create_userdata(ElemWrapper(elem)).unwrap()),
					]))
				} else {
					Ok(mlua::MultiValue::new())
				}
			},
		);
	}
}
