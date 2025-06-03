use std::cell::RefCell;
use std::cell::RefMut;
use std::collections::HashMap;
use std::rc::Rc;

use crate::lua::kernel::Kernel;
use crate::lua::kernel::KernelName;
use crate::unit::translation::CustomData;
use crate::unit::translation::TranslationUnit;

pub static LUA_CUSTOM: &str = "nml.lua.kernel";

/// Data for kernels
pub struct LuaData {
	/// All registered kernels
	pub(crate) registered: HashMap<String, Kernel>,
}

impl LuaData {
	pub(crate) fn initialize(unit: &mut TranslationUnit) {
		if unit.has_data(LUA_CUSTOM) {
			return;
		}

		unit.new_data(Rc::new(RefCell::new(LuaData::default())));
	}

	pub(crate) fn with_kernel<F, R>(unit: &mut TranslationUnit, name: &KernelName, f: F) -> R
	where
		F: FnOnce(&mut TranslationUnit, &Kernel) -> R,
	{
		let kernels = unit.get_data(LUA_CUSTOM);
		let mut kernels = RefMut::map(kernels.borrow_mut(), |b| {
			b.downcast_mut::<LuaData>().unwrap()
		});

		if !kernels.registered.contains_key(&name.0) {
			kernels.registered.insert(name.0.clone(), Kernel::new(unit));
		}
		let kernel = kernels.registered.get(&name.0).unwrap();
		f(unit, kernel)
	}
}

impl Default for LuaData {
	fn default() -> Self {
		Self {
			registered: HashMap::default(),
		}
	}
}

impl CustomData for LuaData {
	fn name(&self) -> &str {
		LUA_CUSTOM
	}
}
