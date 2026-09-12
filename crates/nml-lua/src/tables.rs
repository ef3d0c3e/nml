use mlua::Lua;
use nml_lang::Lower;

/// The LuaCATS declaration of `nml.tables`, for LuaLS.
pub(crate) const DECLARATION: &str = "
---A table named with `:table[lua=name]`: its rows, each a list of its cells' text. A cell
---that spans several rows or columns has its text in each position it covers, and short
---rows are padded with empty text.
---@class nml.Table
---@field header integer How many rows, from the first, are header rows.
---@field [integer] string[]

---The tables named with `:table[lua=name]`, by name, from the point after each table.
---@type table<string, nml.Table>
nml.tables = {}
";

/// Adds the empty `nml.tables`.
pub(crate) fn register(lua: &Lua, api: &mlua::Table) -> mlua::Result<()> {
	api.raw_set("tables", lua.create_table()?)
}

/// Adds the unit's tables that `nml.tables` does not have yet. Tables are only ever added
/// while a unit is lowered, so the ones already there are up to date.
pub(crate) fn sync(lower: &Lower<'_, '_>, lua: &Lua) -> mlua::Result<()> {
	let api: mlua::Table = lua.globals().raw_get("nml")?;
	let tables: mlua::Table = api.raw_get("tables")?;
	for (name, table) in &lower.tables {
		if tables.raw_get::<mlua::Value>(name.as_str())?.is_nil() {
			let value = lua.create_table()?;
			for row in &table.rows {
				value.raw_push(lua.create_sequence_from(row.iter().map(String::as_str))?)?;
			}
			value.raw_set("header", table.header)?;
			tables.raw_set(name.as_str(), value)?;
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;

	use crate::tests::compile;
	use crate::tests::html;

	/// The paragraphs of a page's HTML, without their tags.
	fn paragraphs(text: &str) -> Vec<String> {
		let compiled = compile(text);
		assert_diagnostics!(compiled.diagnostics, []);
		html(&compiled)
			.lines()
			.filter_map(|line| line.strip_prefix("<p>")?.strip_suffix("</p>"))
			.map(String::from)
			.collect()
	}

	#[test]
	fn read_tables() {
		let rows = r#":lua
```
function show(t)
	local rows = {}
	for _, row in ipairs(t) do
		table.insert(rows, table.concat(row, ","))
	end
	return t.header .. ":" .. table.concat(rows, ";")
end
```
"#;
		// Spans repeat their text, short rows are padded, markup is plain text, and the
		// padding of the cells is trimmed.
		let text = format!(
			"{rows}:lua`tostring(nml.tables.m)`\n\n\
			:table[lua=m]\n| a | **b** | c |\n|---|---|---|\n|[vspan=2] 1 |[hspan=2] x |\n| y |\n| z |\n\n\
			:lua`show(nml.tables.m)`"
		);
		assert_eq!(
			paragraphs(&text),
			["nil", "1:a,b,c;1,x,x;1,y,;z,,"]
		);
		// Numbers coerce, and the table works with a reference and a caption.
		let compiled = compile(
			":table[lua=sizes] #sizes Sizes\n| 1.5 | 2 |\n| 3 | 4 |\n\n\
			:lua`nml.tables.sizes[1][1] * nml.tables.sizes[2][2]`",
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let body = html(&compiled);
		assert!(body.contains("<table id=\"sizes\">"), "{body}");
		assert!(body.contains("Table 1:</span> Sizes"), "{body}");
		assert!(body.contains("<p>6.0</p>"), "{body}");
		// Without a separator, no row is a header.
		let text = format!("{rows}:table[lua=t]\n|a|\n\n:lua`show(nml.tables.t)`");
		assert_eq!(paragraphs(&text), ["0:a"]);
	}

	#[test]
	fn tables_in_post_parse() {
		let text = ":lua`nml.post_parse(function() return #nml.tables.late end)`\n\n\
			:table[lua=late]\n|a|\n|b|";
		let compiled = compile(text);
		assert_diagnostics!(compiled.diagnostics, []);
		assert!(html(&compiled).contains("<p>2</p>"), "{}", html(&compiled));
	}
}
