use std::cell::RefMut;

use mlua::Lua;

pub struct Kernel
{
	pub lua: Lua,
}

impl Kernel {
    pub fn new() -> Self {
        Self { lua: Lua::new() }
    }
}

pub trait KernelHolder
{
	fn get_kernel(&self, name: &str) -> Option<RefMut<'_, Kernel>>;

	fn insert_kernel(&self, name: String, kernel: Kernel) -> RefMut<'_, Kernel>;
}
