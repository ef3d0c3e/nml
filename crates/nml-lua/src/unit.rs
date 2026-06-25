use std::cell::RefCell;
use std::rc::Rc;

use nml_diagnostics::Diagnostic;
use nml_lang::Lower;

use crate::Limits;
use crate::Runtime;
use crate::error::Chunks;
use crate::events;
use crate::post_parse;

/// The Lua of the unit being lowered: its state, its chunks, and the errors raised where no
/// diagnostic can be added directly (element listeners).
pub(crate) struct UnitLua {
	pub runtime: Runtime,
	pub chunks: Rc<RefCell<Chunks>>,
	pub errors: Rc<RefCell<Vec<Diagnostic>>>,
	pub pending: post_parse::Pending,
}

/// The unit's Lua, created on first use.
pub(crate) fn unit_lua<'a>(lower: &'a mut Lower<'_, '_>) -> &'a mut UnitLua {
	if lower.extension_mut::<UnitLua>().is_none() {
		let unit = UnitLua {
			runtime: Runtime::new(Limits::default()).expect("a Lua state can be created"),
			chunks: Rc::default(),
			errors: Rc::default(),
			pending: post_parse::Pending::default(),
		};
		let lua = unit.runtime.lua().clone();
		let chunks = unit.chunks.clone();
		let errors = unit.errors.clone();
		lower
			.builder
			.on_write(move |draft| events::dispatch(&lua, draft, &chunks, &errors));
		lower.on_finish(post_parse::fill);
		lower.on_finish(|lower| {
			let errors = std::mem::take(&mut *unit_lua(lower).errors.borrow_mut());
			lower.diagnostics.extend(errors);
		});
		lower.extension(|| unit);
	}
	lower.extension_mut().unwrap()
}
