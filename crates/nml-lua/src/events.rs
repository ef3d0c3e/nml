use std::cell::RefCell;

use mlua::Function;
use mlua::Lua;
use nml_diagnostics::Diagnostic;
use nml_document::Draft;
use nml_document::ElementKind;
use nml_document::ListKind;

use crate::api;
use crate::error::Chunks;
use crate::userdata;

/// The element kinds listeners can ask for.
const KINDS: &[&str] = ElementKind::NAMES;

/// The listeners of a unit, kept in its Lua state.
#[derive(Default)]
struct Listeners(Vec<(String, Function)>);

/// Calls `listener` with every element of `kind` before it is written.
///
/// The listener receives the element (see `nml.Element`). Changing its `id` and `classes`
/// changes the element.
#[api]
fn on(
	lua: &Lua,
	kind: String,
	#[lua(type = "fun(element: nml.Element)")] listener: Function,
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

/// An element about to be written, as listeners see it.
#[derive(Clone)]
#[userdata(name = "nml.Element")]
pub(crate) struct ElementData {
	/// The element's kind: `heading`, `paragraph`, …
	#[lua(readonly)]
	kind: String,
	/// The text below the element, without markup.
	#[lua(readonly)]
	text: String,
	/// The reference name, which is also the HTML `id`.
	id: Option<String>,
	/// The HTML classes. Assign a new list to change them.
	classes: Vec<String>,
	/// A heading's level.
	#[lua(readonly)]
	level: Option<i64>,
	/// A style's name: `bold`, `italic`, …
	#[lua(readonly)]
	style: Option<String>,
	/// Whether a list is numbered.
	#[lua(readonly)]
	numbered: Option<bool>,
	/// A numbered list's first number.
	#[lua(readonly)]
	start: Option<i64>,
	/// A list item's checkbox: `unchecked`, `partial` or `checked`.
	#[lua(readonly)]
	checkbox: Option<String>,
	/// A table's cells, row by row, as text.
	#[lua(readonly)]
	cells: Option<Vec<Vec<String>>>,
}

fn call(lua: &Lua, draft: Draft<'_>, listeners: &[Function]) -> mlua::Result<()> {
	let (level, style) = match draft.kind {
		ElementKind::Heading { level } => (Some(i64::from(*level)), None),
		ElementKind::Style(style) => (None, Some(format!("{style:?}").to_lowercase())),
		_ => (None, None),
	};
	let (numbered, start) = match draft.kind {
		ElementKind::List(ListKind::Bullet { .. }) => (Some(false), None),
		ElementKind::List(ListKind::Numbered { start, .. }) => {
			(Some(true), Some(i64::try_from(*start).unwrap_or(i64::MAX)))
		}
		_ => (None, None),
	};
	let checkbox = match draft.kind {
		ElementKind::ListItem {
			checkbox: Some(checkbox),
			..
		} => Some(format!("{checkbox:?}").to_lowercase()),
		_ => None,
	};
	let cells = matches!(draft.kind, ElementKind::Table { .. }).then(|| {
		draft
			.children()
			.filter(|row| row.kind == ElementKind::TableRow)
			.map(|row| {
				row.children()
					.iter()
					.map(|cell| draft.text_of(*cell))
					.collect()
			})
			.collect()
	});
	let element = lua.create_userdata(ElementData {
		kind: draft.kind.name().into(),
		text: draft.text(),
		id: draft.attributes.id.clone(),
		classes: draft.attributes.classes.clone(),
		level,
		style,
		numbered,
		start,
		checkbox,
		cells,
	})?;
	for listener in listeners {
		listener.call::<()>(&element)?;
	}
	let element = element.borrow::<ElementData>()?;
	draft.attributes.id = element.id.clone();
	draft.attributes.classes = element.classes.clone();
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
	local classes = h.classes
	table.insert(classes, "numbered")
	h.classes = classes
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
	fn lists() {
		let compiled = compile(
			r#":lua
```
seen = {}
nml.on("list", function(l) table.insert(seen, tostring(l.numbered) .. ":" .. tostring(l.start)) end)
nml.on("list_item", function(i) table.insert(seen, tostring(i.checkbox)) end)
```

 * [x] a
 * b

 -[offset=4] c

:lua`table.concat(seen, " ")`
"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let body = html(&compiled);
		assert!(
			body.contains("<p>checked nil false:nil nil true:4</p>"),
			"{body}"
		);
	}

	#[test]
	fn tables() {
		let compiled = compile(
			r#":lua
```
total = 0
nml.on("table", function(t)
	for i, row in ipairs(t.cells) do
		if i > 1 then total = total + tonumber(row[2]) end
	end
end)
```

| Item | Price |
|---|---|
| a | 2 |
| b | **3** |

Total: :lua`nml.post_parse(function() return total end)`
"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert!(
			html(&compiled).contains("<p>Total: 5</p>"),
			"{}",
			html(&compiled)
		);
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

	#[test]
	fn read_only_fields() {
		let compiled =
			compile(":lua\n```\nnml.on('heading', function(h) h.level = 3 end)\n```\n\n# A\n");
		assert_diagnostics!(compiled.diagnostics, [4001]);
		let nml_diagnostics::DiagnosticKind::LuaError { message } = &compiled.diagnostics[0].kind
		else {
			panic!()
		};
		assert!(message.contains("level"), "{message}");
	}

	#[test]
	fn declaration() {
		let declarations = crate::luacats(&[], &[super::ElementData::LUA_CLASS], &[]);
		assert!(
			declarations.contains(
				"---An element about to be written, as listeners see it.
---@class nml.Element
---@field kind string (read-only) The element's kind: `heading`, `paragraph`, …
---@field text string (read-only) The text below the element, without markup.
---@field id string? The reference name, which is also the HTML `id`.
---@field classes string[] The HTML classes. Assign a new list to change them.
---@field level integer? (read-only) A heading's level.
---@field style string? (read-only) A style's name: `bold`, `italic`, …
---@field numbered boolean? (read-only) Whether a list is numbered.
---@field start integer? (read-only) A numbered list's first number.
---@field checkbox string? (read-only) A list item's checkbox: `unchecked`, `partial` or `checked`.
---@field cells string[][]? (read-only) A table's cells, row by row, as text.
"
			),
			"{declarations}"
		);
	}
}
