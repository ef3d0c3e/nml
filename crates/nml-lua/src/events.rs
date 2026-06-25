use std::cell::RefCell;

use mlua::Function;
use mlua::Lua;
use mlua::Table;
use nml_diagnostics::Diagnostic;
use nml_document::Draft;
use nml_document::ElementKind;

use crate::api;
use crate::error::Chunks;

/// The element kinds listeners can ask for.
const KINDS: &[&str] = &[
	"document",
	"paragraph",
	"text",
	"style",
	"heading",
	"slot",
	"reference",
];

/// The listeners of a unit, kept in its Lua state.
#[derive(Default)]
struct Listeners(Vec<(String, Function)>);

/// Calls `listener` with every element of `kind` before it is written.
///
/// The listener receives the element as a table with its `kind`, its `text` (without
/// markup), its `id` and `classes`, and the fields of its kind (`level` for a heading,
/// `style` for a style). Changing `id` and `classes` changes the element.
#[api]
fn on(
	lua: &Lua,
	kind: String,
	#[lua(type = "fun(element: table)")] listener: Function,
) -> mlua::Result<()> {
	if !KINDS.contains(&kind.as_str()) {
		return Err(mlua::Error::runtime(format!(
			"no element kind is named `{kind}`: use one of {}",
			KINDS.join(", ")
		)));
	}
	if lua.app_data_ref::<Listeners>().is_none() {
		lua.set_app_data(Listeners::default());
	}
	lua.app_data_mut::<Listeners>()
		.unwrap()
		.0
		.push((kind, listener));
	Ok(())
}

/// Calls the listeners of the element about to be written. Errors are kept in `errors`.
pub(crate) fn dispatch(
	lua: &Lua,
	draft: Draft<'_>,
	chunks: &RefCell<Chunks>,
	errors: &RefCell<Vec<Diagnostic>>,
) {
	let kind = draft.kind.name();
	let listeners: Vec<Function> = match lua.app_data_ref::<Listeners>() {
		Some(listeners) => listeners
			.0
			.iter()
			.filter(|(k, _)| k == kind)
			.map(|(_, f)| f.clone())
			.collect(),
		None => return,
	};
	if listeners.is_empty() {
		return;
	}
	let token = draft.token.clone();
	if let Err(error) = call(lua, draft, &listeners) {
		errors
			.borrow_mut()
			.push(chunks.borrow().diagnostic(&error, token));
	}
}

fn call(lua: &Lua, draft: Draft<'_>, listeners: &[Function]) -> mlua::Result<()> {
	let element = lua.create_table()?;
	element.set("kind", draft.kind.name())?;
	element.set("text", draft.text())?;
	match draft.kind {
		ElementKind::Heading { level } => element.set("level", *level)?,
		ElementKind::Style(style) => element.set("style", format!("{style:?}").to_lowercase())?,
		_ => {}
	}
	element.set("id", draft.attributes.id.clone())?;
	element.set(
		"classes",
		lua.create_sequence_from(draft.attributes.classes.iter().cloned())?,
	)?;
	for listener in listeners {
		listener.call::<()>(&element)?;
	}

	let id: Option<String> = element
		.get("id")
		.map_err(|_| mlua::Error::runtime("`id` must be a string or nil"))?;
	let classes: Table = element
		.get("classes")
		.map_err(|_| mlua::Error::runtime("`classes` must be a table of strings"))?;
	let classes = classes
		.sequence_values::<String>()
		.collect::<mlua::Result<Vec<_>>>()
		.map_err(|_| mlua::Error::runtime("`classes` must be a table of strings"))?;
	draft.attributes.id = id;
	draft.attributes.classes = classes;
	Ok(())
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;

	use crate::tests::compile;
	use crate::tests::html;

	#[test]
	fn listeners_change_elements() {
		let compiled = compile(
			r#":lua
```
headings = {}
nml.on("heading", function(h)
	table.insert(headings, h.level .. ":" .. h.text)
	h.id = h.id or ("h-" .. #headings)
	table.insert(h.classes, "numbered")
end)
```

# First
## Second {#two}
Count: :lua`#headings`, :lua`table.concat(headings, ", ")`.
"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let body = html(&compiled);
		assert!(
			body.contains("<h1 id=\"h-1\" class=\"numbered\">First</h1>"),
			"{body}"
		);
		assert!(
			body.contains("<h2 id=\"two\" class=\"numbered\">Second</h2>"),
			"{body}"
		);
		assert!(body.contains("Count: 2, 1:First, 2:Second."), "{body}");
	}

	#[test]
	fn listener_errors() {
		let compiled =
			compile(":lua\n```\nnml.on('heading', function(h)\n\terror('no')\nend)\n```\n\n# A\n");
		assert_diagnostics!(compiled.diagnostics, [4001]);
		assert_eq!(
			compiled.diagnostics[0].primary().location.root().content(),
			"error('no')"
		);

		let compiled =
			compile(":lua\n```\nnml.on('heading', function(h) h.classes = 1 end)\n```\n\n# A\n");
		assert_diagnostics!(compiled.diagnostics, [4001]);
		// Reported at the heading, since no line of Lua is to blame.
		assert_eq!(
			compiled.diagnostics[0].primary().location.root().content(),
			"# A\n"
		);

		let compiled = compile(":lua`nml.on('nope', print)`");
		assert_diagnostics!(compiled.diagnostics, [4001]);
	}
}
