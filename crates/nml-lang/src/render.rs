use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::ElementKind;
use nml_document::RenderKind;
use nml_grammar::Atom;
use nml_grammar::OptionKind;
use nml_grammar::OptionSchema;
use nml_grammar::OptionSpec;
use nml_grammar::OptionValue;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::RuleMatch;
use nml_grammar::Value;

use crate::Language;
use crate::Lower;

/// The Graphviz layout engines.
const LAYOUTS: &[&str] = &[
	"dot",
	"neato",
	"fdp",
	"sfdp",
	"circo",
	"twopi",
	"osage",
	"patchwork",
];

/// Source rendered by external programs when the page is built: math `$…$` and `$$…$$`,
/// `:tex[env=…]`, `:graph[layout=…]` and `:typst`, each followed by a fence, and the role
/// ``:typst`…` ``.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	for (name, open, block, doc, snippet) in [
		(
			"math",
			"$",
			false,
			"**Math** in a line, in LaTeX: `$e^{i\\pi} + 1 = 0$`, rendered when the page is built.",
			"\\$${1:x}\\$",
		),
		(
			"display_math",
			"$$",
			true,
			"**Math** on its own line, in LaTeX: `$$\\sum_{i=1}^n i$$`, rendered when the page \
			is built.",
			"\\$\\$${1:x}\\$\\$",
		),
	] {
		let rule = Rule::new(
			name,
			vec![
				Atom::capture("open", Atom::literal(open)).with_class("operator"),
				Atom::capture("source", Atom::Text).with_class("string"),
				Atom::capture("close", Atom::literal(open)).with_class("operator"),
			],
		)
		.with_doc(doc)
		.with_snippet(snippet);
		language.add(rule, move |lower, m| {
			let source = m.capture("source").unwrap().token.content().to_string();
			let kind = ElementKind::Rendered {
				kind: RenderKind::Math,
				source,
				block,
			};
			lower.builder.leaf(kind, m.token.clone());
		})?;
	}

	let tex = OptionSchema::new([OptionSpec::new("env", OptionKind::Text)
		.with_default(OptionValue::Text("main".into()))
		.with_doc("The LaTeX environment of the configuration (`[render.tex.<name>]`).")]);
	let layouts = LAYOUTS.iter().map(|layout| layout.to_string()).collect();
	let graph = OptionSchema::new([OptionSpec::new("layout", OptionKind::Choice(layouts))
		.with_default(OptionValue::Text("dot".into()))
		.with_doc("The Graphviz layout engine.")]);
	for (command, schema, languages, doc) in [
		(
			"tex",
			Some(tex),
			&["tex", "latex"][..],
			"**LaTeX**, rendered to an image when the page is built.\n\n\
			````nml\n:tex[env=main]\n```\n\\begin{tikzpicture} … \\end{tikzpicture}\n```\n````\n\n\
			`env` picks the environment of `[render.tex.<name>]` in `nml.toml`.",
		),
		(
			"graph",
			Some(graph),
			&["dot"][..],
			"**Graph**, in Graphviz's DOT, rendered when the page is built.\n\n\
			````nml\n:graph[layout=dot]\n```\ndigraph { a -> b }\n```\n````\n\n\
			`layout` is the Graphviz engine.",
		),
		(
			"typst",
			None,
			&["typst"][..],
			"**Typst**, rendered when the page is built.\n\n\
			````nml\n:typst\n```\n#rect[Hello]\n```\n````",
		),
	] {
		let snippet = format!(":{command}\n```\n${{0}}\n```");
		let rule = fenced(command, schema).with_doc(doc).with_snippet(snippet);
		language.add(rule, move |lower, m| {
			lower_fenced(lower, m, command, languages)
		})?;
	}

	let role = Rule::new(
		"typst_role",
		vec![
			Atom::capture("command", Atom::literal(":typst")).with_class("keyword"),
			Atom::capture("open", Atom::run('`', 1)).with_class("operator"),
			Atom::capture("source", Atom::Text).with_class("string"),
			Atom::capture("close", Atom::same("open")).with_class("operator"),
		],
	)
	.with_doc("**Typst** in a line, rendered when the page is built: ``` :typst`$a^2$` ```.")
	.with_snippet(":typst`${1}`");
	language.add(role, |lower, m| {
		let kind = ElementKind::Rendered {
			kind: RenderKind::Typst,
			source: m.capture("source").unwrap().token.content().to_string(),
			block: false,
		};
		lower.builder.leaf(kind, m.token.clone());
	})?;
	Ok(())
}

/// `:command[options]`, then a fence with an optional language, the source, and the fence.
fn fenced(command: &str, schema: Option<OptionSchema>) -> Rule {
	let mut atoms = vec![
		Atom::LineStart,
		Atom::capture("command", Atom::literal(format!(":{command}"))).with_class("keyword"),
	];
	if let Some(schema) = schema {
		atoms.push(Atom::optional(vec![Atom::capture(
			"options",
			Atom::Options(Arc::new(schema)),
		)]));
	}
	atoms.extend([
		Atom::LineEnd,
		Atom::LineStart,
		Atom::capture("fence", Atom::run('`', 3)).with_class("operator"),
		Atom::capture("info", Atom::RestOfLine).with_class("type"),
		Atom::LineEnd,
		Atom::capture("source", Atom::Text),
		Atom::LineStart,
		Atom::capture("close", Atom::same("fence")).with_class("operator"),
		Atom::LineEnd,
	]);
	Rule::new(command, atoms)
}

fn lower_fenced(lower: &mut Lower<'_, '_>, m: &RuleMatch, command: &str, languages: &[&str]) {
	let info = &m.capture("info").unwrap().token;
	let language = info.content().trim();
	if !language.is_empty() && !languages.contains(&language) {
		let kind = DiagnosticKind::FenceLanguage {
			language: language.into(),
			expected: languages[0].into(),
		};
		let message = DiagnosticMessage::new(info.clone());
		lower.diagnostics.push(Diagnostic::new(kind, message));
		return;
	}
	let option = |name| match m.capture("options").map(|capture| &capture.value) {
		Some(Value::Options(options)) => match options.get(name) {
			Some(OptionValue::Text(text)) => Some(text.clone()),
			_ => None,
		},
		_ => None,
	};
	let kind = match command {
		"tex" => RenderKind::Tex {
			environment: option("env").unwrap_or_else(|| "main".into()),
		},
		"graph" => RenderKind::Graph {
			layout: option("layout").unwrap_or_else(|| "dot".into()),
		},
		_ => RenderKind::Typst,
	};
	let source = m.capture("source").unwrap().token.content();
	let source = source.strip_suffix('\n').unwrap_or(source);
	let element = ElementKind::Rendered {
		kind,
		source: source.to_string(),
		block: true,
	};
	lower.builder.leaf(element, m.token.clone());
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::ElementKind;
	use nml_document::RenderKind;

	use crate::tests::compile_text;

	fn rendered(text: &str) -> Vec<ElementKind> {
		let compiled = compile_text(text);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		document
			.descendants(document.root())
			.filter(|(_, element)| matches!(element.kind, ElementKind::Rendered { .. }))
			.map(|(_, element)| element.kind.clone())
			.collect()
	}

	fn element(kind: RenderKind, source: &str, block: bool) -> ElementKind {
		ElementKind::Rendered {
			kind,
			source: source.into(),
			block,
		}
	}

	#[test]
	fn math() {
		assert_eq!(
			rendered("Euler: $e^{i\\pi} = -1$, and $$\\sum_i i$$. It costs 5\\$."),
			[
				element(RenderKind::Math, "e^{i\\pi} = -1", false),
				element(RenderKind::Math, "\\sum_i i", true),
			]
		);
		let compiled = compile_text("It costs 5$.");
		assert_diagnostics!(compiled.diagnostics, [1001]);
	}

	#[test]
	fn fenced_sources() {
		assert_eq!(
			rendered(
				":graph[layout=neato]\n```dot\ndigraph { a -> b }\n```\n:tex[env=tikz]\n```\n\\begin{tikzpicture}\\end{tikzpicture}\n```\n:typst\n```typst\n$ a^2 $\n```\nInline :typst`$x$`."
			),
			[
				element(
					RenderKind::Graph {
						layout: "neato".into()
					},
					"digraph { a -> b }",
					true
				),
				element(
					RenderKind::Tex {
						environment: "tikz".into()
					},
					"\\begin{tikzpicture}\\end{tikzpicture}",
					true
				),
				element(RenderKind::Typst, "$ a^2 $", true),
				element(RenderKind::Typst, "$x$", false),
			]
		);
		assert_eq!(
			rendered(":graph\n```\ngraph {}\n```"),
			[element(
				RenderKind::Graph {
					layout: "dot".into()
				},
				"graph {}",
				true
			)]
		);
	}

	#[test]
	fn errors() {
		let compiled = compile_text(":graph\n```mermaid\nx\n```");
		assert_diagnostics!(compiled.diagnostics, [2020 @ 10..17]);
		let compiled = compile_text(":graph[layout=grid]\n```\nx\n```");
		assert_diagnostics!(compiled.diagnostics, [1007]);
	}
}
