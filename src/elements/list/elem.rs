use std::sync::Arc;

use auto_userdata::AutoUserData;
use mlua::AnyUserData;
use mlua::Lua;
use parking_lot::RwLock;
use serde::Deserialize;
use serde::Serialize;
use mlua::serde::LuaSerdeExt;

use crate::lua::wrappers::*;
use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ContainerElement;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum CheckboxState {
	Checked,
	Unchecked,
	Partial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BulletMarker {
	Bullet,
	Checkbox(CheckboxState),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListMarker {
	pub(crate) numbered: bool,
	pub(crate) offset: usize,
}

#[derive(Debug, Clone, AutoUserData)]
#[auto_userdata_target = "*"]
#[auto_userdata_target = "&"]
#[auto_userdata_target = "&mut"]
pub struct ListEntry {
	#[allow(unused)]
	pub(crate) location: Token,
	#[lua_value]
	pub(crate) bullet: BulletMarker,
	#[lua_map(ScopeWrapper)]
	pub(crate) content: Arc<RwLock<Scope>>,
	#[lua_value]
	pub(crate) markers: Vec<ListMarker>,
}

#[derive(Debug, AutoUserData)]
#[auto_userdata_target = "*"]
#[auto_userdata_target = "&"]
#[auto_userdata_target = "&mut"]
pub struct List {
	pub(crate) location: Token,
	#[lua_map(VecScopeWrapper)]
	pub(crate) contained: Vec<Arc<RwLock<Scope>>>,
	#[lua_map(LuaUDVec)]
	pub(crate) entries: Vec<ListEntry>,
}

impl List {
	pub fn add_entry(&mut self, entry: ListEntry) {
		self.contained.push(entry.content.clone());
		self.entries.push(entry);
	}
}

impl Element for List {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> crate::unit::element::ElemKind {
		ElemKind::Block
	}

	fn element_name(&self) -> &'static str {
		"List"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		let mut stack = vec![];

		let match_stack = |stack: &mut Vec<(bool, usize)>,
		                   target: &Vec<ListMarker>,
		                   output: &mut CompilerOutput| {
			// Find first diff index
			let mut m = 0;
			for t in target {
				if stack.len() <= m || stack[m].0 != t.numbered {
					break;
				}
				m += 1;
			}

			// Apply offset
			if m == stack.len() && m != 0 {
				stack[m - 1].1 += target[m - 1].offset;
				return true;
			}

			// Close
			for e in stack[m..].iter().rev() {
				match compiler.target() {
					Target::HTML => output.add_content(["</ul>", "</ol>"][e.0 as usize]),
					_ => todo!(),
				}
			}

			// Open
			for e in target[m..].iter() {
				stack.push((e.numbered, e.offset));
				match compiler.target() {
					Target::HTML => output.add_content(["<ul>", "<ol>"][e.numbered as usize]),
					_ => todo!(),
				}
			}
			false
		};

		for entry in &self.entries {
			let has_offset = match_stack(&mut stack, &entry.markers, output);
			match compiler.target() {
				Target::HTML => {
					if has_offset {
						output.add_content(format!(r#"<li value="{}">"#, stack.last().unwrap().1));
					} else {
						output.add_content("<li>");
					}
					match &entry.bullet {
						BulletMarker::Checkbox(state) => match state {
							CheckboxState::Unchecked => {
								output.add_content(
									r#"<input type="checkbox" class="checkbox-unchecked" onclick="return false;">"#,
								);
							}
							CheckboxState::Partial => {
								output.add_content(
									r#"<input type="checkbox" class="checkbox-partial" onclick="return false;">"#,
								);
							}
							CheckboxState::Checked => {
								output.add_content(
									r#"<input type="checkbox" class="checkbox-checked" onclick="return false;" checked>"#,
								);
							}
						},
						_ => {}
					}
				}
				_ => todo!(),
			}
			for (scope, elem) in entry.content.content_iter(false) {
				elem.compile(scope, compiler, output)?;
			}
			match compiler.target() {
				Target::HTML => output.add_content("</li>"),
				_ => todo!(),
			}
		}
		match_stack(&mut stack, &vec![], output);
		Ok(())
	}

	fn as_container(self: Arc<Self>) -> Option<Arc<dyn ContainerElement>> {
		Some(self)
	}

	fn lua_wrap(self: Arc<Self>, lua: &Lua) -> Option<AnyUserData> {
		let r: &'static _ = unsafe { &*Arc::as_ptr(&self) };
		Some(lua.create_userdata(r).unwrap())
	}
}

impl ContainerElement for List {
	fn contained(&self) -> &[Arc<RwLock<Scope>>] {
		self.contained.as_slice()
	}
}
