use mlua::AnyUserData;
use mlua::Function;
use mlua::Lua;
use mlua::UserData;
use nml_document::PostParseSlot;
use nml_document::Slot;
use nml_document::SlotId;
use nml_lang::Lower;
use nml_source::Token;

use crate::api;
use crate::unit::unit_lua;

/// The value of `nml.post_parse`: shown by a `:lua` role, it becomes a post-parse slot.
pub(crate) struct PostParse(pub u32);

impl UserData for PostParse {}

/// The post-parse callbacks of a unit, kept in its Lua state.
#[derive(Default)]
struct Callbacks(Vec<Function>);

/// The post-parse slots of the unit, waiting for the end of its parse.
#[derive(Default)]
pub(crate) struct Pending(Vec<(SlotId, u32, Token)>);

/// Defers content until the document is parsed: shown by a `:lua` role, the value becomes
/// what `callback` returns (text, elements, or a list of them) once the whole document is
/// parsed, in the same Lua state.
/// A table of contents placed before its headings is the typical use.
#[api(returns = "userdata")]
fn post_parse(
	lua: &Lua,
	#[lua(type = "fun(): any")] callback: Function,
) -> mlua::Result<AnyUserData> {
	if lua.app_data_ref::<Callbacks>().is_none() {
		lua.set_app_data(Callbacks::default());
	}
	let mut callbacks = lua.app_data_mut::<Callbacks>().unwrap();
	callbacks.0.push(callback);
	let id = u32::try_from(callbacks.0.len() - 1).expect("too many post-parse callbacks");
	drop(callbacks);
	lua.create_userdata(PostParse(id))
}

/// Adds the slot of a post-parse callback at `token`.
pub(crate) fn defer(lower: &mut Lower<'_, '_>, callback: u32, token: Token) {
	let slot = Slot::PostParse(PostParseSlot { callback });
	let id = lower.builder.slot(slot, token.clone());
	unit_lua(lower).pending.0.push((id, callback, token));
}

/// Runs the post-parse callbacks and fills their slots. Called when the lowering ends.
pub(crate) fn fill(lower: &mut Lower<'_, '_>) {
	let pending = std::mem::take(&mut unit_lua(lower).pending.0);
	for (slot, callback, token) in pending {
		let lua = unit_lua(lower).runtime.lua().clone();
		let function = lua.app_data_ref::<Callbacks>().unwrap().0[callback as usize].clone();
		let markup = function
			.call(())
			.and_then(crate::elements::nodes)
			.and_then(|elements| crate::elements::html(&elements, Some(lower)));
		match markup {
			Ok(markup) => lower.builder.fill_slot(slot, Slot::Markup(markup)),
			Err(error) => {
				let diagnostic = unit_lua(lower).chunks.borrow().diagnostic(&error, token);
				lower.diagnostics.push(diagnostic);
				lower.builder.fill_slot(slot, Slot::Markup(String::new()));
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;

	use crate::tests::compile;
	use crate::tests::html;

	#[test]
	fn table_of_contents() {
		let compiled = compile(
			r#":lua
```
headings = {}
nml.on("heading", function(h) table.insert(headings, h.text) end)
```

Contents: :lua`nml.post_parse(function() return table.concat(headings, ", ") end)`.

# One
# Two
Count: :lua`nml.post_parse(function() return #headings end)`
"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let body = html(&compiled);
		assert!(body.contains("<p>Contents: One, Two.</p>"), "{body}");
		assert!(body.contains("<p>Count: 2</p>"), "{body}");
		// Nothing is left for the link stage.
		assert!(
			nml_html::compile(&compiled.unit.document, &Default::default())
				.slots
				.is_empty()
		);
	}

	#[test]
	fn callback_errors() {
		let compiled = compile(":lua`nml.post_parse(function() error('late') end)`\n");
		assert_diagnostics!(compiled.diagnostics, [4001]);
		assert_eq!(
			compiled.diagnostics[0].primary().location.root().content(),
			"nml.post_parse(function() error('late') end)"
		);
		let compiled = compile(":lua`nml.post_parse(function() return true end)`\n");
		assert_diagnostics!(compiled.diagnostics, [4001]);
	}

	#[test]
	fn escaped_as_text() {
		let compiled = compile(":lua`nml.post_parse(function() return '<b>' end)`\n");
		assert!(html(&compiled).contains("<p>&lt;b&gt;</p>"));
	}

	#[test]
	fn elements() {
		let compiled = compile(
			":lua`nml.post_parse(function() return nml.element('list', { 'a', nml.element('raw', { content = '<br>' }) }) end)`\n",
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let html = html(&compiled);
		assert!(
			html.contains("<ul>\n<li>a</li>\n<li><br></li>\n</ul>\n"),
			"{html}"
		);
		// References need the link stage, which post-parse output skips.
		let compiled = compile(
			":lua`nml.post_parse(function() return nml.element('reference', { target = 'x' }) end)`\n",
		);
		assert_diagnostics!(compiled.diagnostics, [4001]);
	}
}
