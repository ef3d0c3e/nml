use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::Dependency;
use nml_grammar::Atom;
use nml_grammar::Rule;
use nml_grammar::RuleError;

use crate::Import;
use crate::Language;
use crate::ResolveError;

/// `:import foo.bar` and `:import foo.bar as name`, in the prologue.
///
/// The module becomes a parse-time dependency, under a namespace: the alias, or the last
/// segment of its path. What a module exports (Lua, rules, `:export` variables) comes with
/// those features.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let rule = Rule::new(
		"import",
		vec![
			Atom::LineStart,
			Atom::capture("command", Atom::literal(":import ")).with_class("keyword"),
			Atom::capture("module", Atom::Text).with_class("namespace"),
			Atom::optional(vec![
				Atom::capture("as", Atom::literal(" as ")).with_class("keyword"),
				Atom::capture("alias", Atom::Word).with_class("namespace"),
			]),
			Atom::LineEnd,
		],
	);
	language.add_prologue(rule, |lower, m| {
		let module = m.capture("module").unwrap().token.clone();
		let path = module.content();
		let report = |kind| Diagnostic::new(kind, DiagnosticMessage::new(module.clone()));
		if !lower.prologue {
			let kind = DiagnosticKind::ImportOutsidePrologue {
				module: path.into(),
			};
			lower.diagnostics.push(report(kind));
			return;
		}
		if !is_module_path(path) {
			let kind = DiagnosticKind::InvalidModulePath {
				module: path.into(),
			};
			lower.diagnostics.push(report(kind));
			return;
		}
		let (file, source) = match lower.resolver.module(path) {
			Ok(found) => found,
			Err(error) => {
				let kind = match error {
					ResolveError::UnknownModule { name } => DiagnosticKind::UnknownModule { name },
					ResolveError::NotFound { path } => DiagnosticKind::FileNotFound { path },
				};
				lower.diagnostics.push(report(kind));
				return;
			}
		};

		let (namespace, token) = match m.capture("alias") {
			Some(alias) => (alias.token.content().to_string(), alias.token.clone()),
			None => (path.rsplit('.').next().unwrap().to_string(), module.clone()),
		};
		if let Some((_, first)) = lower.imports.iter().find(|(name, _)| *name == namespace) {
			let kind = DiagnosticKind::DuplicateImport { namespace };
			let diagnostic = Diagnostic::new(kind, DiagnosticMessage::new(token))
				.with(DiagnosticMessage::fluent(first.clone(), "first"));
			lower.diagnostics.push(diagnostic);
			return;
		}
		lower.imports.push((namespace.clone(), token));
		lower.depend(Dependency::Import(path.into()));
		let import = Import {
			module: path.into(),
			namespace,
			path: file,
			source,
			token: module.clone(),
		};
		for hook in lower.language().import_hooks() {
			hook(lower, &import);
		}
	})?;
	Ok(())
}

/// At least two dot-separated words: a module name, then the path under its root.
fn is_module_path(path: &str) -> bool {
	let segments: Vec<_> = path.split('.').collect();
	segments.len() >= 2
		&& segments.iter().all(|segment| {
			let mut chars = segment.chars();
			chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
				&& chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-')
		})
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::Dependency;

	use crate::tests::compile_with;

	fn files() -> Vec<(&'static str, &'static str)> {
		vec![
			("modules/std/toc.nml", ""),
			("modules/std/nav/menu.nml", ""),
		]
	}

	#[test]
	fn imports() {
		let compiled = compile_with(
			":import std.toc\n:set title = A\n:import std.nav.menu as nav\n# Title\n",
			&files(),
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert_eq!(
			compiled.unit.dependencies,
			[
				Dependency::Import("std.toc".into()),
				Dependency::Import("std.nav.menu".into())
			]
		);
	}

	#[test]
	fn import_errors() {
		for (text, id, range) in [
			// After content: the prologue has ended.
			("text\n\n:import std.toc", 2008, 14..21),
			("# T\n:import std.toc", 2008, 12..19),
			("[[x]]\n\n:import std.toc", 2008, 15..22),
			// Not a module path.
			(":import toc", 2010, 8..11),
			(":import std..toc", 2010, 8..16),
			// Unknown module or file.
			(":import nope.toc", 3007, 8..16),
			(":import std.missing", 3008, 8..19),
			// The same namespace twice.
			(":import std.toc\n:import std.nav.menu as toc", 2009, 40..43),
		] {
			let compiled = compile_with(text, &files());
			nml_diagnostics::testing::check(&compiled.diagnostics, &[(id, Some(range))]);
		}
	}
}
