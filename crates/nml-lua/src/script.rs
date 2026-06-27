use std::sync::Arc;

use mlua::Value;
use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::Dependency;
use nml_document::ElementKind;
use nml_document::Slot;
use nml_grammar::Atom;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_lang::Language;
use nml_lang::Lower;
use nml_source::Source;
use nml_source::Token;
use nml_source::Url;
use nml_source::VirtualSourceBuilder;

use crate::deferred::Defer;
use crate::post_parse;
use crate::post_parse::PostParse;
use crate::unit::unit_lua;

/// A chunk for the code at `code`, in the file at `path`: a virtual source of it, preceded by
/// `prefix`, registered under a name made of the file and the offset.
pub(crate) fn chunk(
	lower: &mut Lower<'_, '_>,
	path: &str,
	code: &Token,
	prefix: &str,
) -> (String, Arc<Source>) {
	let name = format!("{path}@{}", code.start());
	let url = Url::parse(&format!("virtual:///{path}/lua/{}", code.start())).unwrap();
	let mut builder = VirtualSourceBuilder::new(code.source().clone(), url, code.start());
	builder.insert(prefix);
	builder.copy(code.range());
	let source = Arc::new(builder.build());
	unit_lua(lower)
		.chunks
		.borrow_mut()
		.add(name.clone(), source.clone());
	(name, source)
}

/// `:lua` and a bare (or `lua`) fence runs the code in the unit's state; ``:lua`expr` ``
/// shows the value of `expr`, or runs statements and shows nothing.
pub fn add(language: &mut Language) -> Result<(), RuleError> {
	crate::module::add(language);
	let block = Rule::new(
		"lua",
		vec![
			Atom::LineStart,
			Atom::capture("command", Atom::literal(":lua")).with_class("keyword"),
			Atom::LineEnd,
			Atom::LineStart,
			Atom::capture("fence", Atom::run('`', 3)).with_class("operator"),
			Atom::capture("info", Atom::RestOfLine),
			Atom::LineEnd,
			Atom::capture("code", Atom::Text),
			Atom::LineStart,
			Atom::same("fence"),
			Atom::LineEnd,
		],
	);
	language.add_prologue(block, |lower, m| {
		let info = &m.capture("info").unwrap().token;
		if !matches!(info.content().trim(), "" | "lua") {
			let kind = DiagnosticKind::LuaFenceLanguage {
				language: info.content().trim().into(),
			};
			let message = DiagnosticMessage::new(info.clone());
			lower.diagnostics.push(Diagnostic::new(kind, message));
			return;
		}
		let code = m.capture("code").unwrap().token.clone();
		let path = lower.files.last().unwrap().clone();
		let (name, source) = chunk(lower, &path, &code, "");
		let unit = unit_lua(lower);
		if let Err(error) = unit.runtime.run(source.content(), &name) {
			let diagnostic = unit.chunks.borrow().diagnostic(&error, m.token.clone());
			lower.diagnostics.push(diagnostic);
		}
		crate::rules::register(lower, &m.token);
	})?;

	let role = Rule::new(
		"lua-role",
		vec![
			Atom::capture("command", Atom::literal(":lua")).with_class("keyword"),
			Atom::capture("fence", Atom::run('`', 1)).with_class("operator"),
			Atom::capture("code", Atom::Text),
			Atom::same("fence"),
		],
	);
	language.add(role, |lower, m| {
		let code = m.capture("code").unwrap().token.clone();
		let evaluated = evaluate(lower, &code);
		crate::rules::register(lower, &m.token);
		match evaluated {
			Ok(Value::UserData(data)) if data.is::<Defer>() => {
				let slot = data.borrow::<Defer>().unwrap().0.clone();
				let imported = Dependency::Import(slot.module.clone());
				if lower.dependencies.contains(&imported) {
					lower.builder.slot(Slot::Deferred(slot), m.token.clone());
				} else {
					let kind = DiagnosticKind::DeferredModuleNotImported {
						module: slot.module,
					};
					let message = DiagnosticMessage::new(m.token.clone());
					lower.diagnostics.push(Diagnostic::new(kind, message));
				}
			}
			Ok(Value::UserData(data)) if data.is::<PostParse>() => {
				let callback = data.borrow::<PostParse>().unwrap().0;
				post_parse::defer(lower, callback, m.token.clone());
			}
			Ok(value) => match to_text(value, "a `:lua` role") {
				Ok(Some(text)) => {
					lower.builder.leaf(ElementKind::Text(text), m.token.clone());
				}
				Ok(None) => {}
				Err(error) => {
					let diagnostic = unit_lua(lower)
						.chunks
						.borrow()
						.diagnostic(&error, m.token.clone());
					lower.diagnostics.push(diagnostic);
				}
			},
			Err(error) => {
				let diagnostic = unit_lua(lower)
					.chunks
					.borrow()
					.diagnostic(&error, m.token.clone());
				lower.diagnostics.push(diagnostic);
			}
		}
	})?;
	Ok(())
}

/// Evaluates a role's code: as an expression if it is one, as statements otherwise.
fn evaluate(lower: &mut Lower<'_, '_>, code: &Token) -> mlua::Result<Value> {
	let path = lower.files.last().unwrap().clone();
	let (name, source) = chunk(lower, &path, code, "return ");
	let function = match unit_lua(lower).runtime.load(source.content(), &name) {
		Ok(function) => function,
		Err(mlua::Error::SyntaxError { .. }) => {
			let (name, source) = chunk(lower, &path, code, "");
			unit_lua(lower).runtime.load(source.content(), &name)?
		}
		Err(error) => return Err(error),
	};
	function.call(())
}

/// The text of a value shown in the document: strings and numbers, or nothing for `nil`.
/// `what` names what shows it, for the error.
pub(crate) fn to_text(value: Value, what: &str) -> mlua::Result<Option<String>> {
	match value {
		Value::Nil => Ok(None),
		Value::String(text) => Ok(Some(text.to_str()?.to_string())),
		Value::Integer(n) => Ok(Some(n.to_string())),
		Value::Number(n) => Ok(Some(value_text(n))),
		other => Err(mlua::Error::runtime(format!(
			"{what} shows strings and numbers, not a {}",
			other.type_name()
		))),
	}
}

/// A float as Lua shows it.
fn value_text(n: f64) -> String {
	if n.fract() == 0.0 && n.is_finite() {
		format!("{n:.1}")
	} else {
		n.to_string()
	}
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_lang::Compiled;

	use super::*;
	use crate::tests::compile;

	fn text(compiled: &Compiled) -> String {
		let document = &compiled.unit.document;
		document.text(document.root())
	}

	#[test]
	fn blocks_and_roles() {
		let compiled = compile(
			":lua\n```\nfunction year() return 2026 end\nname = 'NML'\n```\n\n\
			Copyright :lua`year()` :lua``name .. '`'``, :lua`1.5 * 2`, :lua`0.5`.",
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert_eq!(text(&compiled), "Copyright 2026 NML`, 3.0, 0.5.");
	}

	#[test]
	fn roles_can_run_statements() {
		let compiled = compile(":lua`x = 1` :lua`x = x + 1` :lua`x`");
		assert_diagnostics!(compiled.diagnostics, []);
		assert_eq!(text(&compiled), "  2");
		let compiled = compile(":lua`nil`.");
		assert_eq!(text(&compiled), ".");
	}

	#[test]
	fn errors_point_into_the_document() {
		let compiled = compile(":lua\n```\nlocal a = 1\nlocal b = nil + a\n```\n");
		assert_diagnostics!(compiled.diagnostics, [4001]);
		let location = compiled.diagnostics[0].primary().location.root();
		assert_eq!(location.content(), "local b = nil + a");
		assert_eq!(location.source().location().as_str(), "file:///page.nml");

		let compiled = compile("a :lua`1 +` b");
		assert_diagnostics!(compiled.diagnostics, [4001]);
		assert_eq!(
			compiled.diagnostics[0].primary().location.root().content(),
			"1 +"
		);

		let compiled = compile("a :lua`{}` b");
		assert_diagnostics!(compiled.diagnostics, [4001]);
		let DiagnosticKind::LuaError { message } = &compiled.diagnostics[0].kind else {
			panic!()
		};
		assert!(message.contains("not a table"), "{message}");
	}

	#[test]
	fn fence_language() {
		let compiled = compile(":lua\n```lua\nx = 1\n```\n");
		assert_diagnostics!(compiled.diagnostics, []);
		let compiled = compile(":lua\n```rust\nx = 1\n```\n");
		assert_diagnostics!(compiled.diagnostics, [4002 @ 8..12]);
	}

	#[test]
	fn state_is_per_unit() {
		compile(":lua`shared = 1`");
		let compiled = compile(":lua`shared`");
		assert_eq!(text(&compiled), "");
	}
}
