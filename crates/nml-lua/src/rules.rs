use std::cell::RefCell;
use std::collections::HashMap;

use mlua::AnyUserData;
use mlua::Function;
use mlua::Lua;
use mlua::Table;
use mlua::Value;
use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::ElementKind;
use nml_document::Style;
use nml_grammar::Atom;
use nml_grammar::Rule;
use nml_grammar::RuleMatch;
use nml_lang::AddRuleError;
use nml_lang::Lower;
use nml_source::Token;

use crate::api;
use crate::unit::unit_lua;
use crate::userdata;

/// An atom of a rule defined in Lua. Strings stand for literals.
#[derive(Clone)]
#[userdata(name = "nml.Atom")]
pub(crate) struct LuaAtom {
	#[lua(skip)]
	atom: Atom,
}

/// The atoms that are values of the `nml` table, with their documentation.
pub(crate) const VALUES: &[(&str, &str)] = &[
	(
		"inline",
		"Parsed inline content up to the delimiter that follows, within a paragraph.",
	),
	(
		"content",
		"Parsed blocks up to the delimiter that follows, at the start of a line.",
	),
	("text", "Verbatim text up to the delimiter that follows."),
	("rest_of_line", "Verbatim text up to the end of the line."),
	(
		"word",
		"A word: a letter or `_`, then letters, digits, `_` and `-`.",
	),
	("number", "A decimal number, signed or not."),
	("line_start", "The start of a line."),
	("line_end", "The end of a line."),
];

fn value_atom(name: &str) -> Atom {
	match name {
		"inline" => Atom::InlineContent,
		"content" => Atom::Content,
		"text" => Atom::Text,
		"rest_of_line" => Atom::RestOfLine,
		"word" => Atom::Word,
		"number" => Atom::Number {
			signed: true,
			decimal: true,
		},
		"line_start" => Atom::LineStart,
		"line_end" => Atom::LineEnd,
		_ => unreachable!("listed in VALUES"),
	}
}

/// Sets the value atoms in the `nml` table.
pub(crate) fn register_values(table: &Table) -> mlua::Result<()> {
	for (name, _) in VALUES {
		table.raw_set(
			*name,
			LuaAtom {
				atom: value_atom(name),
			},
		)?;
	}
	Ok(())
}

/// An atom from Lua: a string is a literal.
fn atom(value: Value) -> mlua::Result<Atom> {
	match value {
		Value::String(text) => Ok(Atom::literal(text.to_str()?.to_string())),
		Value::UserData(data) if data.is::<LuaAtom>() => Ok(data.borrow::<LuaAtom>()?.atom.clone()),
		other => Err(mlua::Error::runtime(format!(
			"an atom is a string or an `nml` atom, not a {}",
			other.type_name()
		))),
	}
}

fn atoms(table: Table) -> mlua::Result<Vec<Atom>> {
	table
		.sequence_values::<Value>()
		.map(|value| atom(value?))
		.collect()
}

fn wrap(lua: &Lua, atom: Atom) -> mlua::Result<AnyUserData> {
	lua.create_userdata(LuaAtom { atom })
}

/// Names a part of the match: the handler finds it in `m.captures`. `class` is its semantic
/// token class for editors.
#[api(returns = "nml.Atom")]
fn capture(
	lua: &Lua,
	name: String,
	#[lua(type = "string|nml.Atom")] inner: Value,
	class: Option<String>,
) -> mlua::Result<AnyUserData> {
	let mut atom = Atom::capture(name, atom(inner)?);
	if let Some(class) = class {
		atom = atom.with_class(class);
	}
	wrap(lua, atom)
}

/// The atoms, zero or one time.
#[api(returns = "nml.Atom")]
fn optional(
	lua: &Lua,
	#[lua(type = "(string|nml.Atom)[]")] atoms_: Table,
) -> mlua::Result<AnyUserData> {
	wrap(lua, Atom::optional(atoms(atoms_)?))
}

/// The atoms, repeated while their first atom matches: at least `min` times (default 0), at
/// most `max`.
#[api(returns = "nml.Atom")]
fn many(
	lua: &Lua,
	#[lua(type = "(string|nml.Atom)[]")] atoms_: Table,
	min: Option<i64>,
	max: Option<i64>,
) -> mlua::Result<AnyUserData> {
	let bound =
		|n: i64| usize::try_from(n).map_err(|_| mlua::Error::runtime("bounds are not negative"));
	let min = bound(min.unwrap_or(0))?;
	let max = max.map(bound).transpose()?;
	wrap(lua, Atom::repeat(atoms(atoms_)?, min, max))
}

/// The first alternative whose first atom matches.
#[api(returns = "nml.Atom")]
fn one_of(
	lua: &Lua,
	#[lua(type = "(string|nml.Atom)[][]")] alternatives: Table,
) -> mlua::Result<AnyUserData> {
	let alternatives = alternatives
		.sequence_values::<Table>()
		.map(|alternative| atoms(alternative?))
		.collect::<mlua::Result<Vec<_>>>()?;
	wrap(lua, Atom::or(alternatives))
}

/// A run of at least `min` times the character `char`, such as a code fence.
#[api(returns = "nml.Atom")]
fn run(lua: &Lua, char: String, min: i64) -> mlua::Result<AnyUserData> {
	let mut chars = char.chars();
	let (Some(c), None) = (chars.next(), chars.next()) else {
		return Err(mlua::Error::runtime("`run` takes a single character"));
	};
	let min = usize::try_from(min).map_err(|_| mlua::Error::runtime("`min` is not negative"))?;
	wrap(lua, Atom::run(c, min))
}

/// Exactly the text of the earlier capture `name`; for a captured run, a run of the same
/// length.
#[api(returns = "nml.Atom")]
fn same(lua: &Lua, name: String) -> mlua::Result<AnyUserData> {
	wrap(lua, Atom::same(name))
}

/// Rules defined but not yet added to the unit's grammar, and the handlers of those added.
#[derive(Default)]
struct LuaRules {
	pending: Vec<(Rule, Function)>,
	handlers: HashMap<String, Function>,
}

/// Defines a rule of the grammar: `name`, its `atoms` (strings are literals), and the
/// `handler` that builds its elements from the match `m` with `out`. Rules are defined in
/// the prologue or in modules; the document's body is parsed with them.
///
/// `m.text` is the matched text, and `m.captures` the text of each capture by name. A
/// capture inside `nml.many` is a list of texts, empty if it never matched.
#[api]
fn rule(
	lua: &Lua,
	#[lua(
		type = "{ name: string, atoms: (string|nml.Atom)[], handler: fun(m: table, out: table) }"
	)]
	definition: Table,
) -> mlua::Result<()> {
	let name: String = definition
		.get("name")
		.map_err(|_| mlua::Error::runtime("a rule needs a `name` string"))?;
	let atoms_: Table = definition
		.get("atoms")
		.map_err(|_| mlua::Error::runtime("a rule needs a table of `atoms`"))?;
	let handler: Function = definition
		.get("handler")
		.map_err(|_| mlua::Error::runtime("a rule needs a `handler` function"))?;
	let rule = Rule::new(name, atoms(atoms_)?);
	if lua.app_data_ref::<LuaRules>().is_none() {
		lua.set_app_data(LuaRules::default());
	}
	lua.app_data_mut::<LuaRules>()
		.unwrap()
		.pending
		.push((rule, handler));
	Ok(())
}

/// Adds the rules defined by the Lua that just ran to the unit's grammar. Errors are
/// reported at `token`, what ran the Lua.
pub(crate) fn register(lower: &mut Lower<'_, '_>, token: &Token) {
	let lua = unit_lua(lower).runtime.lua().clone();
	let pending = match lua.app_data_mut::<LuaRules>() {
		Some(mut rules) => std::mem::take(&mut rules.pending),
		None => return,
	};
	for (rule, function) in pending {
		let name = rule.name.clone();
		let handler_name = name.clone();
		let result = lower.add_rule(rule, move |lower, m| handle(lower, m, &handler_name));
		let kind = match result {
			Ok(_) => {
				lua.app_data_mut::<LuaRules>()
					.unwrap()
					.handlers
					.insert(name, function);
				continue;
			}
			Err(AddRuleError::OutsidePrologue) => {
				DiagnosticKind::LuaRuleOutsidePrologue { rule: name }
			}
			Err(AddRuleError::Rule(error)) => DiagnosticKind::InvalidLuaRule {
				rule: name,
				message: error.to_string(),
			},
		};
		lower
			.diagnostics
			.push(Diagnostic::new(kind, DiagnosticMessage::new(token.clone())));
	}
}

/// The handler of a rule defined in Lua.
fn handle(lower: &mut Lower<'_, '_>, m: &RuleMatch, name: &str) {
	let lua = unit_lua(lower).runtime.lua().clone();
	let function = lua.app_data_ref::<LuaRules>().unwrap().handlers[name].clone();
	if let Err(error) = call(&lua, lower, m, &function) {
		let diagnostic = unit_lua(lower)
			.chunks
			.borrow()
			.diagnostic(&error, m.token.clone());
		lower.diagnostics.push(diagnostic);
	}
}

fn style(name: &str) -> mlua::Result<Style> {
	Ok(match name {
		"bold" => Style::Bold,
		"italic" => Style::Italic,
		"underline" => Style::Underline,
		"highlight" => Style::Highlight,
		"strikethrough" => Style::Strikethrough,
		_ => return Err(mlua::Error::runtime(format!("no style is named `{name}`"))),
	})
}

/// Calls a Lua handler with the match and an `out` builder that writes into the document.
fn call(
	lua: &Lua,
	lower: &mut Lower<'_, '_>,
	m: &RuleMatch,
	function: &Function,
) -> mlua::Result<()> {
	// A capture inside a repeated part is a list of texts, even with one or no match.
	let rule = lower.language().rules().get(m.rule).unwrap();
	let repeated: Vec<String> = rule
		.repeated_captures()
		.into_iter()
		.map(String::from)
		.collect();
	let captures = lua.create_table()?;
	for name in &repeated {
		captures.raw_set(name.as_str(), lua.create_table()?)?;
	}
	for capture in &m.captures {
		if repeated.iter().any(|name| **name == *capture.name) {
			let list: Table = captures.raw_get(&*capture.name)?;
			list.raw_push(capture.token.content())?;
		} else {
			captures.raw_set(&*capture.name, capture.token.content())?;
		}
	}
	let matched = lua.create_table()?;
	matched.raw_set("text", m.token.content())?;
	matched.raw_set("captures", captures)?;

	let lower = RefCell::new(lower);
	let token = &m.token;
	let borrow = || {
		lower.try_borrow_mut().map_err(|_| {
			mlua::Error::runtime("`out` is busy: it cannot be used from a nested handler")
		})
	};
	// Opens an element, runs `body`, and closes it even if `body` fails.
	let within = |kind: ElementKind, body: Function| -> mlua::Result<()> {
		borrow()?.builder.open(kind, token.clone());
		let result = body.call::<()>(());
		borrow()?.builder.close();
		result
	};
	lua.scope(|scope| {
		let out = lua.create_table()?;
		out.raw_set(
			"text",
			scope.create_function(|_, (_, text): (Value, String)| {
				borrow()?
					.builder
					.leaf(ElementKind::Text(text), token.clone());
				Ok(())
			})?,
		)?;
		out.raw_set(
			"style",
			scope.create_function(|_, (_, name, body): (Value, String, Function)| {
				within(ElementKind::Style(style(&name)?), body)
			})?,
		)?;
		out.raw_set(
			"paragraph",
			scope.create_function(|_, (_, body): (Value, Function)| {
				within(ElementKind::Paragraph, body)
			})?,
		)?;
		out.raw_set(
			"heading",
			scope.create_function(|_, (_, level, body): (Value, u8, Function)| {
				if !(1..=6).contains(&level) {
					return Err(mlua::Error::runtime("a heading's level is 1 to 6"));
				}
				within(ElementKind::Heading { level }, body)
			})?,
		)?;
		out.raw_set(
			"add",
			scope.create_function(|_, (_, value): (Value, Value)| {
				crate::script::show(*borrow()?, value, token)
			})?,
		)?;
		out.raw_set(
			"element",
			scope.create_function(
				|lua, (_, kind, spec, body): (Value, String, Option<Table>, Option<Function>)| {
					let spec = match spec {
						Some(spec) => spec,
						None => lua.create_table()?,
					};
					let element = crate::elements::build(&kind, &spec)?;
					let Some(body) = body else {
						crate::elements::lower(*borrow()?, &[element], token);
						return Ok(());
					};
					// The body writes after the element's children, inside it.
					crate::elements::open(*borrow()?, &element, token);
					let result = body.call::<()>(());
					borrow()?.builder.close();
					result
				},
			)?,
		)?;
		out.raw_set(
			"content",
			scope.create_function(|_, (_, _, name): (Value, Value, String)| {
				let capture = m.capture(&name).ok_or_else(|| {
					mlua::Error::runtime(format!("the match has no capture `{name}`"))
				})?;
				let mut lower = borrow()?;
				match &capture.value {
					nml_grammar::Value::Nodes(nodes) => lower.nodes(nodes),
					_ => {
						let text = capture.token.content().to_string();
						lower
							.builder
							.leaf(ElementKind::Text(text), capture.token.clone());
					}
				}
				Ok(())
			})?,
		)?;
		function.call::<()>((matched, out))
	})
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;

	use crate::tests::compile;
	use crate::tests::compile_with;
	use crate::tests::html;

	const KBD: &str = r#":lua
```
nml.rule {
	name = "kbd",
	atoms = { ":kbd{", nml.capture("keys", nml.inline, "string"), "}" },
	handler = function(m, out)
		out:style("bold", function() out:content(m, "keys") end)
		out:text(" (" .. #m.captures.keys .. ")")
	end,
}
```
"#;

	#[test]
	fn rules_from_the_prologue() {
		let compiled = compile(&format!("{KBD}\nPress :kbd{{Ctrl + *C*}}.\n"));
		assert_diagnostics!(compiled.diagnostics, []);
		assert!(
			html(&compiled).contains("<p>Press <strong>Ctrl + <em>C</em></strong> (10).</p>"),
			"{}",
			html(&compiled)
		);
	}

	#[test]
	fn rules_build_elements() {
		let compiled = compile(
			r##":lua
```
nml.rule {
	name = "kbd",
	atoms = { ":kbd{", nml.capture("keys", nml.inline), "}" },
	handler = function(m, out)
		out:element("code", { code = "#", classes = { "count" } })
		out:element("link", { url = "#keys", id = "k" }, function()
			out:content(m, "keys")
			out:add({ " ", nml.element("italic", { "!" }) })
		end)
	end,
}
```

Press :kbd{Ctrl + **C**}.
"##,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let html = html(&compiled);
		assert!(
			html.contains(
				"<p>Press <code class=\"count\">#</code><a id=\"k\" href=\"#keys\">Ctrl + <strong>C</strong> <em>!</em></a>.</p>"
			),
			"{html}"
		);
	}

	#[test]
	fn rules_from_modules() {
		let compiled = compile_with(
			":import std.kbd\n\nPress :kbd{X}.\n",
			&[("modules/std/kbd.nml", KBD)],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert!(html(&compiled).contains("<p>Press <strong>X</strong> (1).</p>"));
	}

	#[test]
	fn block_rules() {
		let compiled = compile(
			r#":lua
```
nml.rule {
	name = "note",
	atoms = { nml.line_start, "!!! ", nml.capture("title", nml.rest_of_line), nml.line_end },
	handler = function(m, out)
		out:heading(3, function() out:text("Note: ") out:content(m, "title") end)
	end,
}
```

!!! Careful
text
"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert!(
			html(&compiled).contains("<h3>Note: Careful</h3>\n<p>text</p>"),
			"{}",
			html(&compiled)
		);
	}

	#[test]
	fn composite_atoms() {
		let compiled = compile(
			r##":lua
```
nml.rule {
	name = "tags",
	atoms = {
		"#{",
		nml.many({ nml.capture("tag", nml.word), nml.optional({ ", " }) }, 1),
		nml.many({ "!", nml.capture("none", nml.word) }),
		nml.one_of({ { "}" }, { "|", nml.capture("extra", nml.text), "}" } }),
	},
	handler = function(m, out)
		out:text("[" .. table.concat(m.captures.tag, " ") .. "/" .. #m.captures.none .. "]")
	end,
}
```

Tags: #{a, b, c} #{d|x}
"##,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		// Repeated captures are lists, empty when nothing matched.
		assert!(
			html(&compiled).contains("<p>Tags: [a b c/0] [d/0]</p>"),
			"{}",
			html(&compiled)
		);
	}

	#[test]
	fn errors() {
		// Outside the prologue.
		let compiled = compile(
			"text\n\n:lua`nml.rule { name = 'x', atoms = { '@' }, handler = function() end }`\n",
		);
		assert_diagnostics!(compiled.diagnostics, [4003]);

		// An invalid rule: no opener, or one taken by a built-in rule.
		for atoms in ["nml.word", "'**', nml.inline, '**'"] {
			let compiled = compile(&format!(
				":lua\n```\nnml.rule {{ name = 'x', atoms = {{ {atoms} }}, handler = function() end }}\n```\n"
			));
			assert_diagnostics!(compiled.diagnostics, [4004]);
		}

		// Bad definitions are Lua errors where they are written.
		let compiled = compile(
			":lua\n```\nnml.rule { name = 'x', atoms = { 1 }, handler = function() end }\n```\n",
		);
		assert_diagnostics!(compiled.diagnostics, [4001]);
		let compiled = compile(":lua\n```\nnml.rule { name = 'x', atoms = { '@' } }\n```\n");
		assert_eq!(
			compiled.diagnostics[0].kind,
			nml_diagnostics::DiagnosticKind::LuaError {
				message: "a rule needs a `handler` function".into()
			}
		);

		// A handler error, reported at the match; its element is still closed.
		let compiled = compile(
			":lua\n```\nnml.rule { name = 'x', atoms = { '@' }, handler = function(m, out)\n\tout:style('bold', function() error('inside') end)\nend }\n```\n\na @ b\n",
		);
		assert_diagnostics!(compiled.diagnostics, [4001]);
		assert!(html(&compiled).contains("<p>a <strong></strong> b</p>"));
	}
}
