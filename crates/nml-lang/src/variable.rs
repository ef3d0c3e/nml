use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::Export;
use nml_document::Scope;
use nml_document::SymbolKind;
use nml_document::SymbolRef;
use nml_grammar::Atom;
use nml_grammar::Node;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::RuleMatch;
use nml_source::Url;
use nml_source::VirtualSourceBuilder;

use crate::Language;
use crate::Lower;
use crate::Variable;
use crate::language::plain_text;
use crate::resolve::is_output_path;

/// The variable that sets a unit's reference key.
pub(crate) const REFERENCE_KEY: &str = "nml.reference_key";

/// The variable that sets where a unit's page is written in the output directory.
pub(crate) const OUTPUT: &str = "nml.output";

/// The variables NML itself reads; other `nml.` names are errors.
const BUILTIN: &[&str] = &[REFERENCE_KEY, OUTPUT];

/// `:set name = value` and `:export name = value`, and `%name%`, which stands for the value
/// of a variable, parsed as inline markup where it is substituted.
///
/// An exported variable is also a symbol of the unit (its title is the value's text), and
/// the units that import it as a module see it as `%namespace.name%`.
///
/// `:path name = path` and `:export-path name = path` set a path, resolved from the file
/// that sets it: `%name%` stands for it as text, and in the paths of media, links and
/// includes.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	for (rule_name, command, export, path, doc) in [
		(
			"set",
			":set ",
			false,
			false,
			"**Variable**, set in the prologue: `:set name = value`.\n\n\
			`%name%` stands for the value, parsed as markup where it is used. \
			`nml.reference_key` sets the unit's name in references, and `nml.output` where \
			its page is written.",
		),
		(
			"export",
			":export ",
			true,
			false,
			"**Exported variable**, set in the prologue: `:export name = value`.\n\n\
			Like `:set`, and also a symbol of the unit. The units that import this one as a \
			module see it as `%namespace.name%`.",
		),
		(
			"path",
			":path ",
			false,
			true,
			"**Path variable**: `:path logo = img/logo.png`.\n\n\
			The path is resolved from this file, so `%logo%` stands for the same file \
			wherever it is used: in text, and in the paths of media, links and includes \
			(`![Logo](%logo%)`).",
		),
		(
			"export_path",
			":export-path ",
			true,
			true,
			"**Exported path variable**: `:export-path logo = img/logo.png`.\n\n\
			Like `:path`, and seen by the units that import this one as a module as \
			`%namespace.logo%`, still pointing to the same file.",
		),
	] {
		let rule = Rule::new(
			rule_name,
			vec![
				Atom::LineStart,
				Atom::capture("command", Atom::literal(command)).with_class("keyword"),
				Atom::capture("name", Atom::Text).with_class("variable"),
				Atom::literal(" = "),
				Atom::capture("value", Atom::RestOfLine).with_class("string"),
				Atom::LineEnd,
			],
		)
		.with_doc(doc)
		.with_snippet(format!("{command}${{1:name}} = ${{0:value}}"));
		language.add_prologue(rule, move |lower, m| set(lower, m, export, path))?;
	}

	let substitution = Rule::new(
		"substitution",
		vec![
			Atom::capture("open", Atom::literal("%")).with_class("operator"),
			Atom::capture("name", Atom::Text).with_class("variable"),
			Atom::capture("close", Atom::literal("%")).with_class("operator"),
		],
	)
	.with_doc(
		"**Variable**: `%name%` stands for the value of the variable `name`, parsed as markup \
		here. Set it with `:set name = value`; an imported module's are `%namespace.name%`.",
	)
	.with_snippet("%${1:name}%");
	language.add(substitution, substitute)?;

	language.on_import(|lower, import| {
		// The module's `:export` lines, at the top of the file.
		let rules = lower.language().rules().clone();
		let parsed = nml_grammar::parse(&rules, import.source.clone());
		for node in &parsed.nodes {
			let Node::Match(m) = node else { continue };
			let path = match rules.get(m.rule).map(|rule| rule.name.as_str()) {
				Some("export") => false,
				Some("export_path") => true,
				_ => continue,
			};
			if !m.complete {
				continue;
			}
			let name = m.capture("name").unwrap().token.clone();
			let mut value = m.capture("value").unwrap().token.clone();
			if path {
				// Resolved from the module's file; its errors are the module's.
				let Some(resolved) = resolved_path(&import.path, &value) else {
					continue;
				};
				value = resolved;
			}
			if is_variable_name(name.content()) {
				let key = format!("{}.{}", import.namespace, name.content());
				lower.variables.insert(key, Variable { name, value, path });
			}
		}
	});
	Ok(())
}

/// A path written at `value` in the file at `from`, resolved, in a virtual source of its
/// own that maps to `value`: `/` then its workspace path. `None` if it leaves the workspace.
fn resolved_path(from: &str, value: &nml_source::Token) -> Option<nml_source::Token> {
	let resolved = crate::resolve_file(from, value.content().trim())?;
	let (root, offset) = value
		.source()
		.root_offset(value.start(), nml_source::Bias::Start);
	let mut location = Url::parse("virtual:///").unwrap();
	location.set_path(&format!("{}/path/{offset}", root.location().path()));
	let mut builder = VirtualSourceBuilder::new(value.source().clone(), location, value.start());
	builder.insert(&format!("/{resolved}"));
	let source = Arc::new(builder.build());
	let len = source.content().len();
	Some(nml_source::Token::new(source, 0..len))
}

/// `text` with every `%name%` of a variable that is set replaced by its value as written
/// (a path variable's, resolved). Other `%…%` stay, since URLs hold `%` escapes.
pub(crate) fn expand_paths(lower: &Lower<'_, '_>, text: &str) -> String {
	let mut out = String::new();
	let mut rest = text;
	while let Some(start) = rest.find('%') {
		out.push_str(&rest[..start]);
		let after = &rest[start + 1..];
		let variable = after
			.find('%')
			.map(|end| &after[..end])
			.filter(|name| is_variable_name(name))
			.and_then(|name| Some((name, lower.variables.get(name)?)));
		match variable {
			Some((name, variable)) => {
				out.push_str(variable.value.content().trim());
				rest = &after[name.len() + 1..];
			}
			None => {
				out.push('%');
				rest = after;
			}
		}
	}
	out.push_str(rest);
	out
}

fn set(lower: &mut Lower<'_, '_>, m: &RuleMatch, export: bool, path: bool) {
	let name = m.capture("name").unwrap().token.clone();
	let mut value = m.capture("value").unwrap().token.clone();
	let error = |kind| Diagnostic::new(kind, DiagnosticMessage::new(name.clone()));
	let text = name.content();
	if !is_variable_name(text) {
		let kind = DiagnosticKind::InvalidVariableName { name: text.into() };
		lower.diagnostics.push(error(kind));
		return;
	}
	if text.starts_with("nml.") && (export || !BUILTIN.contains(&text)) {
		let kind = DiagnosticKind::UnknownVariable { name: text.into() };
		lower.diagnostics.push(error(kind));
		return;
	}
	if let Some(first) = lower.variables.get(text) {
		let kind = DiagnosticKind::VariableAlreadySet { name: text.into() };
		let first = DiagnosticMessage::fluent(first.name.clone(), "first");
		lower.diagnostics.push(error(kind).with(first));
		return;
	}
	if text == REFERENCE_KEY
		&& SymbolRef::parse(value.content()).is_none_or(|r| r.scope != Scope::Local)
	{
		let kind = DiagnosticKind::InvalidReferenceKey {
			key: value.content().into(),
		};
		lower
			.diagnostics
			.push(Diagnostic::new(kind, DiagnosticMessage::new(value)));
		return;
	}
	if text == OUTPUT && !is_output_path(value.content()) {
		let kind = DiagnosticKind::InvalidOutputPath {
			path: value.content().into(),
		};
		lower
			.diagnostics
			.push(Diagnostic::new(kind, DiagnosticMessage::new(value)));
		return;
	}
	if path {
		let from = lower.files.last().unwrap().clone();
		match resolved_path(&from, &value) {
			Some(resolved) => value = resolved,
			None => {
				let kind = DiagnosticKind::FileNotFound {
					path: value.content().trim().into(),
				};
				lower
					.diagnostics
					.push(Diagnostic::new(kind, DiagnosticMessage::new(value)));
				return;
			}
		}
	}
	if export {
		let title = match path {
			true => value.content().to_string(),
			false => {
				let source = value_source(&value);
				plain_text(&nml_grammar::parse_inline(lower.language().rules(), source).nodes)
			}
		};
		let export = Export::new(text, SymbolKind::Variable, None)
			.with("title", title)
			.with("value", value.content());
		lower.export(export, name.clone());
	}
	lower
		.variables
		.insert(text.into(), Variable { name, value, path });
}

/// The variables being substituted, innermost last: a variable whose value refers to itself
/// would never end.
#[derive(Default)]
struct Substituting(Vec<String>);

fn substitute(lower: &mut Lower<'_, '_>, m: &RuleMatch) {
	let name = m.capture("name").unwrap().token.clone();
	let error = |kind| Diagnostic::new(kind, DiagnosticMessage::new(name.clone()));
	let Some(variable) = lower.variables.get(name.content()).cloned() else {
		let kind = DiagnosticKind::UndefinedVariable {
			name: name.content().into(),
		};
		lower.diagnostics.push(error(kind));
		return;
	};
	let substituting = lower.extension(Substituting::default);
	if substituting.0.iter().any(|n| n == name.content()) {
		let kind = DiagnosticKind::VariableCycle {
			name: name.content().into(),
		};
		lower.diagnostics.push(error(kind));
		return;
	}
	if variable.path {
		let text = variable.value.content().to_string();
		lower
			.builder
			.leaf(nml_document::ElementKind::Text(text), m.token.clone());
		return;
	}
	substituting.0.push(name.content().into());
	let parsed = nml_grammar::parse_inline(lower.language().rules(), value_source(&variable.value));
	lower.diagnostics.extend(parsed.diagnostics);
	lower.nodes(&parsed.nodes);
	lower.extension(Substituting::default).0.pop();
}

/// A variable's value as a source of its own, whose positions map to where it is written.
fn value_source(value: &nml_source::Token) -> Arc<nml_source::Source> {
	let (root, offset) = value
		.source()
		.root_offset(value.start(), nml_source::Bias::Start);
	let mut location = Url::parse("virtual:///").unwrap();
	location.set_path(&format!("{}/variable/{offset}", root.location().path()));
	let mut builder = VirtualSourceBuilder::new(value.source().clone(), location, value.start());
	builder.copy(value.range());
	Arc::new(builder.build())
}

/// Dot-separated words: `nml.reference_key`, `title`.
fn is_variable_name(name: &str) -> bool {
	name.split('.').all(|part| {
		let mut chars = part.chars();
		chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
			&& chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-')
	})
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;

	use crate::tests::check;
	use crate::tests::compile_text;
	use crate::tests::compile_with;

	#[test]
	fn compiled_variables() {
		let compiled = compile_with(
			":import std.lib\n:set nml.output = x.html\n:set name = **Bob**\n\nHi %name%.\n",
			&[("modules/std/lib.nml", ":export site = Docs\n")],
		);
		let mut variables: Vec<_> = (compiled.variables.iter())
			.map(|(key, v)| (key.as_str(), v.name.content(), v.value.content()))
			.collect();
		variables.sort();
		assert_eq!(
			variables,
			[
				("lib.site", "site", "Docs"),
				("name", "name", "**Bob**"),
				("nml.output", "nml.output", "x.html"),
			]
		);
	}

	#[test]
	fn output() {
		let compiled = compile_text(":set nml.output = guide/index.html\n\n# A");
		assert_diagnostics!(compiled.diagnostics, []);
		let output = compiled.unit.output.unwrap();
		assert_eq!(output.content(), "guide/index.html");
		assert_eq!(output.range(), 18..34);
		assert_eq!(compile_text("# A").unit.output, None);
	}

	#[test]
	fn reference_key() {
		let compiled =
			compile_text(":set nml.reference_key = setup\n:set title = Install NML\n# A");
		assert_diagnostics!(compiled.diagnostics, []);
		assert_eq!(compiled.unit.reference_key.as_deref(), Some("setup"));

		let compiled = compile_text("# A");
		assert_eq!(compiled.unit.reference_key, None);
	}

	#[test]
	fn substitution() {
		let compiled = check(
			":set version = **2.1**\n:set name = NML %version%\n\n# %name%\nUse %name%, 50\\%.",
			r#"document[h1["NML " Bold["2.1"]] paragraph["Use " "NML " Bold["2.1"] ", 50%."]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		// A variable is set where it is written: it can be set after the prologue.
		check(
			"a\n\n:set x = y\n%x%",
			r#"document[paragraph["a"] paragraph["y"]]"#,
		);
	}

	#[test]
	fn substitution_errors() {
		let compiled = compile_text("%nope% and %%");
		assert_diagnostics!(compiled.diagnostics, [2015 @ 1..5, 2015 @ 12..12]);
		// Errors in a value are reported where it is written.
		let compiled = compile_text(":set x = *a\n%x%");
		assert_diagnostics!(compiled.diagnostics, [1001]);
		let location = compiled.diagnostics[0].primary().location.root();
		assert_eq!(location.range(), 9..10);
		assert_eq!(location.source().location().as_str(), "file:///page.nml");
		let compiled = compile_text(":set a = %b%\n:set b = %a%\n%a%");
		assert_diagnostics!(compiled.diagnostics, [2016]);
		let location = compiled.diagnostics[0].primary().location.root();
		assert_eq!(location.range(), 23..24);
		// An unescaped `%` opens a substitution.
		let compiled = compile_text("50% off");
		assert_diagnostics!(compiled.diagnostics, [1001]);
	}

	#[test]
	fn exports() {
		let compiled = check(
			":export version = *2.1*\n%version%",
			r#"document[paragraph[Italic["2.1"]]]"#,
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let export = &compiled.unit.exports[0];
		assert_eq!(export.name, "version");
		assert_eq!(export.property("title"), Some("2.1"));
		assert_eq!(export.property("value"), Some("*2.1*"));

		let compiled = compile_text(":export nml.reference_key = x");
		assert_diagnostics!(compiled.diagnostics, [2005]);
	}

	#[test]
	fn module_variables() {
		let compiled = compile_with(
			":import std.meta\n:import std.meta as m\n%meta.version% %m.version%",
			&[(
				"modules/std/meta.nml",
				":export version = **3**\n:set private = x\n",
			)],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		assert_eq!(
			crate::tests::show(document, document.root()),
			r#"document[paragraph[Bold["3"] " " Bold["3"]]]"#
		);
		let compiled = compile_with(
			":import std.meta\n%meta.private%",
			&[("modules/std/meta.nml", ":set private = x\n")],
		);
		assert_diagnostics!(compiled.diagnostics, [2015]);
	}

	#[test]
	fn errors() {
		for (text, expected) in [
			(":set nml.colour = x", vec![(2005, 5..15)]),
			(":set 1x = y", vec![(2004, 5..7)]),
			(":set a..b = y", vec![(2004, 5..9)]),
			(":set nml.reference_key = a b", vec![(2007, 25..28)]),
			(":set nml.reference_key = a#b", vec![(2007, 25..28)]),
			(":set nml.output = ../a.html", vec![(2021, 18..27)]),
			(":set nml.output = a.md", vec![(2021, 18..22)]),
			(":export nml.output = a.html", vec![(2005, 8..18)]),
			(":set x = 1\n:set x = 2", vec![(2006, 16..17)]),
		] {
			let compiled = compile_text(text);
			let expected: Vec<_> = expected.into_iter().map(|(id, r)| (id, Some(r))).collect();
			nml_diagnostics::testing::check(&compiled.diagnostics, &expected);
		}
		// A missing ` = ` leaves the name unclosed.
		let compiled = compile_text(":set x");
		assert_diagnostics!(compiled.diagnostics, [1001 @ 0..5]);
	}

	#[test]
	fn path_variables() {
		let assets = |compiled: &crate::Compiled| -> Vec<String> {
			compiled
				.unit
				.assets
				.iter()
				.map(|asset| asset.path.clone())
				.collect()
		};
		// Resolved from the file that sets it, here an included one.
		let compiled = crate::tests::compile_with(
			":include parts/p\n\nAt %logo%: ![L](%logo%)",
			&[
				("parts/p.nml", ":path logo = img/logo.png\n"),
				("parts/img/logo.png", "png"),
			],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert_eq!(assets(&compiled), ["parts/img/logo.png"]);
		let text = compiled.unit.document.text(compiled.unit.document.root());
		assert!(text.contains("At /parts/img/logo.png: "), "{text}");

		// Exported by a module, resolved from the module's file.
		let compiled = crate::tests::compile_with(
			":import std.brand\n\n![B](%brand.logo%)\n\n:include %brand.part%",
			&[
				(
					"modules/std/brand.nml",
					":export-path logo = img/b.png\n:export-path part = parts/x\n",
				),
				("modules/std/img/b.png", "png"),
				("modules/std/parts/x.nml", "Included."),
			],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert_eq!(assets(&compiled), ["modules/std/img/b.png"]);
		let text = compiled.unit.document.text(compiled.unit.document.root());
		assert!(text.contains("Included."), "{text}");

		// A path is text, not markup; exported, its title is the path.
		let compiled = compile_text(":export-path p = a_*b*\n\n%p%");
		assert_diagnostics!(compiled.diagnostics, []);
		let text = compiled.unit.document.text(compiled.unit.document.root());
		assert_eq!(text, "/a_*b*");
		assert_eq!(compiled.unit.exports[0].property("title"), Some("/a_*b*"));

		// Other variables work in paths too; `%` escapes of URLs stay.
		let compiled = compile_text(
			":set site = https://x.y\n\n[a](%site%/a) [b](https://x.y/?q=%E2%82%AC%nope%)",
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let urls: Vec<_> = compiled
			.unit
			.document
			.descendants(compiled.unit.document.root())
			.filter_map(|(_, element)| match &element.kind {
				nml_document::ElementKind::Link { url } => Some(url.clone()),
				_ => None,
			})
			.collect();
		assert_eq!(urls, ["https://x.y/a", "https://x.y/?q=%E2%82%AC%nope%"]);

		// A path that leaves the workspace.
		let compiled = compile_text(":path out = ../x\n");
		assert_diagnostics!(compiled.diagnostics, [3008 @ 12..16]);
	}
}
