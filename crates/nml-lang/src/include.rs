use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::Dependency;
use nml_grammar::Atom;
use nml_grammar::Rule;
use nml_grammar::RuleError;

use crate::Language;
use crate::ResolveError;

/// `:include path`: the file is parsed at this point, as part of the including unit. It
/// sees the includer's context (grammar, variables), and what it declares belongs to the
/// includer. It is parsed as its own source, so it cannot close what the includer opened,
/// nor leave something open.
///
/// The path is relative to the including file, or to the workspace root with a leading `/`;
/// `.nml` may be left out.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let rule = Rule::new(
		"include",
		vec![
			Atom::LineStart,
			Atom::capture("command", Atom::literal(":include ")).with_class("keyword"),
			Atom::capture("path", Atom::RestOfLine).with_class("string"),
			Atom::LineEnd,
		],
	);
	language.add(rule, |lower, m| {
		let token = m.capture("path").unwrap().token.clone();
		let report = |kind| Diagnostic::new(kind, DiagnosticMessage::new(token.clone()));
		let from = lower.files.last().unwrap().clone();
		let (path, source) = match lower.resolver.include(&from, token.content().trim_end()) {
			Ok(found) => found,
			Err(ResolveError::NotFound { path } | ResolveError::UnknownModule { name: path }) => {
				lower
					.diagnostics
					.push(report(DiagnosticKind::FileNotFound { path }));
				return;
			}
		};
		lower.depend(Dependency::Include(path.clone()));
		if lower.files.contains(&path) {
			lower
				.diagnostics
				.push(report(DiagnosticKind::IncludeCycle { path }));
			return;
		}

		let parsed = nml_grammar::parse(lower.language().rules(), source);
		lower.diagnostics.extend(parsed.diagnostics);
		lower.files.push(path);
		lower.nodes(&parsed.nodes);
		lower.files.pop();
	})?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::Dependency;

	use crate::tests::compile_with;
	use crate::tests::show;

	#[test]
	fn includes() {
		let compiled = compile_with(
			"# Page\n:include parts/intro\nafter\n",
			&[
				("parts/intro.nml", "## Intro {#intro}\n:include ../shared\n"),
				("shared.nml", "Shared **text**.\n"),
			],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		assert_eq!(
			show(document, document.root()),
			r#"document[h1["Page"] h2#intro["Intro"] paragraph["Shared " Bold["text"] "."] paragraph["after"]]"#
		);
		// What the included file declares belongs to the includer.
		assert_eq!(compiled.unit.exports[0].name, "intro");
		assert_eq!(
			compiled.unit.dependencies,
			[
				Dependency::Include("parts/intro.nml".into()),
				Dependency::Include("shared.nml".into())
			]
		);
	}

	#[test]
	fn includes_see_the_includers_context() {
		// Both copies declare `x` in the same unit: a duplicate, reported in the included
		// file.
		let compiled = compile_with(":include a\n:include a\n", &[("a.nml", "# A {#x}\n")]);
		assert_diagnostics!(compiled.diagnostics, [2002 @ 6..7]);
		assert_eq!(
			compiled.diagnostics[0]
				.primary()
				.location
				.source()
				.location()
				.as_str(),
			"file:///a.nml"
		);
	}

	#[test]
	fn includes_are_balanced() {
		// The included `**` is unclosed in its own file; the includer's text after the
		// include is not part of it. The includer is parsed first, so its error comes first.
		let compiled = compile_with(":include a\nb**\n", &[("a.nml", "x **y\n")]);
		assert_diagnostics!(compiled.diagnostics, [1001 @ 12..14, 1001 @ 2..4]);
		let file = |index: usize| {
			let location = &compiled.diagnostics[index].primary().location;
			location.source().location().to_string()
		};
		assert_eq!(file(0), "file:///page.nml");
		assert_eq!(file(1), "file:///a.nml");
	}

	#[test]
	fn includes_end_the_prologue() {
		let compiled = compile_with(
			":include a\n:import std.toc\n",
			&[("a.nml", ""), ("modules/std/toc.nml", "")],
		);
		assert_diagnostics!(compiled.diagnostics, [2008 @ 19..26]);
	}

	#[test]
	fn include_errors() {
		let compiled = compile_with(":include nope\n", &[]);
		assert_diagnostics!(compiled.diagnostics, [3008 @ 9..13]);
		let compiled = compile_with(":include ../../x\n", &[]);
		assert_diagnostics!(compiled.diagnostics, [3008 @ 9..16]);

		// A cycle through two files, and a file including itself.
		let compiled = compile_with(
			":include a\n",
			&[("a.nml", ":include b\n"), ("b.nml", ":include a\n")],
		);
		assert_diagnostics!(compiled.diagnostics, [3009 @ 9..10]);
		let compiled = compile_with(":include page\n", &[("page.nml", "")]);
		assert_diagnostics!(compiled.diagnostics, [3009 @ 9..13]);

		// An import in an included file is outside the prologue.
		let compiled = compile_with(
			":include a\n",
			&[("a.nml", ":import std.toc\n"), ("modules/std/toc.nml", "")],
		);
		assert_diagnostics!(compiled.diagnostics, [2008 @ 8..15]);
	}
}
