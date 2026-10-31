use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::Attributes;
use nml_document::ElementId;
use nml_document::ElementKind;
use nml_document::Export;
use nml_document::SymbolKind;
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
use crate::language::plain_text;

/// Code blocks: a fence of three or more backticks, the language, options and a title, the
/// code, and the same fence.
///
/// ````text
/// ```rust[line_offset=10] Counting characters
/// fn count(s: &str) -> usize { s.chars().count() }
/// ```
/// ````
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let schema = OptionSchema::new([
		OptionSpec::new("line_offset", OptionKind::Integer)
			.with_default(OptionValue::Integer(1))
			.with_doc("The number of the first line."),
		OptionSpec::new("numbers", OptionKind::Bool)
			.with_default(OptionValue::Bool(true))
			.with_doc("Whether lines are numbered."),
		OptionSpec::new("max_lines", OptionKind::Integer)
			.with_default(OptionValue::Integer(0))
			.with_doc("How many lines are shown before the rest scrolls; `0` shows them all."),
	]);
	let rule = Rule::new(
		"code_block",
		vec![
			Atom::LineStart,
			Atom::capture("fence", Atom::run('`', 3)).with_class("operator"),
			Atom::optional(vec![
				Atom::capture("language", Atom::Word).with_class("type"),
			]),
			Atom::optional(vec![Atom::capture(
				"options",
				Atom::Options(Arc::new(schema)),
			)]),
			// A name makes it a numbered listing.
			Atom::optional(vec![
				Atom::literal(" #"),
				Atom::capture("ref", Atom::Word).with_class("variable"),
			]),
			Atom::optional(vec![
				Atom::literal(" "),
				Atom::capture("title", Atom::InlineContent),
			]),
			Atom::LineEnd,
			Atom::capture("code", Atom::Text),
			Atom::LineStart,
			Atom::capture("close", Atom::same("fence")).with_class("operator"),
			Atom::LineEnd,
		],
	)
	.with_doc(
		"**Code block**, highlighted in its language, with an optional title.\n\n\
		````nml\n```rust[line_offset=10] main.rs\nfn main() {}\n```\n````\n\n\
		Options: `line_offset` (the number of the first line), `numbers` (whether lines are \
		numbered), `max_lines` (how many lines are shown before the rest scrolls). A longer fence lets the code hold ```` ``` ````.\n\n\
		A name before the title makes it a numbered listing: ```` ```rust #main The entry point ```` \
		shows *Listing 1: The entry point*, and `[[main]]` shows *Listing 1*.",
	)
	.with_snippet("```${1:language} ${2:Title}\n${0}\n```");
	language.add(rule, |lower, m| {
		let language = m.capture("language").map(|capture| capture.token.clone());
		if let Some(language) = &language {
			check_language(lower, language);
		}
		let options = match m.capture("options").map(|capture| &capture.value) {
			Some(Value::Options(options)) => Some(options),
			_ => None,
		};
		let option = |name| options.and_then(|options| options.get(name));
		let numbered = !matches!(option("numbers"), Some(OptionValue::Bool(false)));
		let first_line = match option("line_offset") {
			Some(OptionValue::Integer(first)) => *first,
			_ => 1,
		};
		let max_lines = match option("max_lines") {
			Some(OptionValue::Integer(0)) | None => None,
			Some(OptionValue::Integer(lines)) if *lines > 0 => {
				Some(u32::try_from(*lines).unwrap_or(u32::MAX))
			}
			_ => {
				let entry = options.and_then(|options| {
					options
						.entries
						.iter()
						.find(|e| e.key.content() == "max_lines")
				});
				let location = entry
					.and_then(|entry| entry.value_token.clone())
					.unwrap_or_else(|| m.token.clone());
				let kind = DiagnosticKind::InvalidOptionValue {
					name: "max_lines".into(),
					expected: "Positive".into(),
				};
				lower
					.diagnostics
					.push(Diagnostic::new(kind, DiagnosticMessage::new(location)));
				None
			}
		};
		let code = m.capture("code").unwrap().token.content();
		// The line break before the closing fence ends the last line; it is not part of it.
		let code = code
			.strip_suffix('\n')
			.map(|code| code.strip_suffix('\r').unwrap_or(code))
			.unwrap_or(code);
		let reference = m.capture("ref").map(|capture| capture.token.clone());
		let number = reference.as_ref().map(|_| next_listing(lower));
		let kind = ElementKind::CodeBlock {
			language: language.map(|language| language.content().to_string()),
			code: code.to_string(),
			first_line: numbered.then_some(first_line),
			max_lines,
			number,
		};
		let attributes = Attributes {
			id: reference.as_ref().map(|r| r.content().to_string()),
			classes: Vec::new(),
		};
		lower.builder.open_with(kind, m.token.clone(), attributes);
		lower.capture(m, "title");
		let element = lower.builder.close();
		if let (Some(reference), Some(number)) = (reference, number) {
			let title = match m.capture("title").map(|capture| &capture.value) {
				Some(Value::Nodes(nodes)) => plain_text(nodes),
				_ => String::new(),
			};
			let export = listing_export(reference.content(), element, number, title);
			lower.export(export, reference);
		}
	})?;
	listing(language)
}

/// `:listing[file=…, lines=…]` or `[file=…, region=…]`: code from a file of the workspace,
/// shown as a code block. The page depends on the file.
fn listing(language: &mut Language) -> Result<(), RuleError> {
	let text = |name: &str, doc: &str| {
		OptionSpec::new(name, OptionKind::Text)
			.with_default(OptionValue::Text(String::new()))
			.with_doc(doc)
	};
	let schema = OptionSchema::new([
		OptionSpec::new("file", OptionKind::Text).with_doc(
			"The file, relative to the document, or to the workspace root with a leading `/`.",
		),
		text(
			"lines",
			"The lines shown: `10-20`, `10-` or `-20`, from 1. Default: all.",
		),
		text(
			"region",
			"The lines between those holding `nml:begin <region>` and `nml:end <region>`, \
			in any comment.",
		),
		text(
			"language",
			"The language of the code. Default: the file's extension.",
		),
		OptionSpec::new("numbers", OptionKind::Bool)
			.with_default(OptionValue::Bool(true))
			.with_doc("Whether lines are numbered, from their number in the file."),
		OptionSpec::new("max_lines", OptionKind::Integer)
			.with_default(OptionValue::Integer(0))
			.with_doc("How many lines are shown before the rest scrolls; `0` shows them all."),
	]);
	let rule = Rule::new(
		"listing",
		vec![
			Atom::LineStart,
			Atom::capture("command", Atom::literal(":listing")).with_class("keyword"),
			Atom::capture("options", Atom::Options(Arc::new(schema))),
			Atom::optional(vec![
				Atom::literal(" #"),
				Atom::capture("ref", Atom::Word).with_class("variable"),
			]),
			Atom::optional(vec![
				Atom::literal(" "),
				Atom::capture("title", Atom::InlineContent),
			]),
			Atom::LineEnd,
		],
	)
	.with_doc(
		"**Listing from a file**: code from a file of the workspace, kept in sync with it.\n\n\
		```nml\n:listing[file=src/main.rs, lines=10-20] #main The entry point\n```\n\n\
		`region=name` shows the lines between `nml:begin name` and `nml:end name` instead. \
		Lines are numbered from their number in the file; `language`, `numbers` and \
		`max_lines` are as a code block's.",
	)
	.with_snippet(":listing[file=${1:path}] ${0}");
	language.add(rule, lower_listing)?;
	Ok(())
}

/// The lines `start..end` (from 0) that `lines` selects among `count`.
fn line_range(lines: &str, count: usize) -> Option<std::ops::Range<usize>> {
	let (start, end) = match lines.split_once('-') {
		Some((start, end)) => (start.trim(), end.trim()),
		None => (lines.trim(), lines.trim()),
	};
	let number = |text: &str, default: usize| match text {
		"" => Some(default),
		text => text.parse::<usize>().ok(),
	};
	let start = number(start, 1)?;
	let end = number(end, count)?;
	(1 <= start && start <= end && end <= count).then(|| start - 1..end)
}

/// The lines of a region: those between the line holding `nml:begin <name>` and the one
/// holding `nml:end <name>`.
fn region(lines: &[&str], name: &str) -> Option<std::ops::Range<usize>> {
	let marks = |marker: &str| {
		let marker = format!("nml:{marker} {name}");
		move |line: &&str| {
			line.find(&marker).is_some_and(|at| {
				let after = &line[at + marker.len()..];
				after
					.chars()
					.next()
					.is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '-')
			})
		}
	};
	let begin = lines.iter().position(marks("begin"))?;
	let end = begin + 1 + lines[begin + 1..].iter().position(marks("end"))?;
	Some(begin + 1..end)
}

/// Lines without the indentation they share.
fn dedent(lines: &[&str]) -> String {
	let indent = (lines.iter())
		.filter(|line| !line.trim().is_empty())
		.map(|line| line.len() - line.trim_start().len())
		.min()
		.unwrap_or(0);
	(lines.iter())
		.map(|line| line.get(indent..).unwrap_or(line.trim_start()))
		.collect::<Vec<_>>()
		.join("\n")
}

fn lower_listing(lower: &mut Lower<'_, '_>, m: &RuleMatch) {
	let options = match m.capture("options").map(|capture| &capture.value) {
		Some(Value::Options(options)) => options,
		_ => return,
	};
	let options_token = m.capture("options").unwrap().token.clone();
	let text = |name| match options.get(name) {
		Some(OptionValue::Text(text)) => text.clone(),
		_ => String::new(),
	};
	let error = |lower: &mut Lower<'_, '_>, kind| {
		let message = DiagnosticMessage::new(options_token.clone());
		lower.diagnostics.push(Diagnostic::new(kind, message));
	};
	// Without `file`, the options are in error already.
	if text("file").is_empty() {
		return;
	}
	let path = crate::variable::expand_paths(lower, &text("file"));
	let from = lower.files.last().unwrap().clone();
	let (resolved, content) = match lower.resolver.read(&from, &path) {
		Ok(file) => file,
		Err(_) => return error(lower, DiagnosticKind::FileNotFound { path }),
	};
	let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
	lower.depend(nml_document::Dependency::File {
		path: resolved.clone(),
		hash,
	});
	let all: Vec<&str> = content.lines().collect();
	let (lines, region_name) = (text("lines"), text("region"));
	let selected = match (lines.is_empty(), region_name.is_empty()) {
		(false, false) => return error(lower, DiagnosticKind::ListingSelection),
		(true, true) => 0..all.len(),
		(false, true) => match line_range(&lines, all.len()) {
			Some(range) => range,
			None => {
				let count = all.len();
				return error(lower, DiagnosticKind::ListingLines { lines, count });
			}
		},
		(true, false) => match region(&all, &region_name) {
			Some(range) => range,
			None => {
				let kind = DiagnosticKind::UnknownRegion {
					region: region_name,
					path: resolved,
				};
				return error(lower, kind);
			}
		},
	};
	let first_line = selected.start as i64 + 1;
	let code = dedent(&all[selected]);
	let language = match text("language") {
		language if !language.is_empty() => Some(language),
		_ => (resolved.rsplit('/').next())
			.and_then(|name| name.rsplit_once('.'))
			.map(|(_, extension)| extension.to_string()),
	};
	if let Some(language) = &language
		&& !nml_render::is_known_language(language)
	{
		let kind = DiagnosticKind::UnknownCodeLanguage {
			language: language.clone(),
		};
		lower.diagnostics.push(Diagnostic::new(
			kind,
			DiagnosticMessage::new(options_token.clone()),
		));
	}
	let numbered = !matches!(options.get("numbers"), Some(OptionValue::Bool(false)));
	let max_lines = match options.get("max_lines") {
		Some(OptionValue::Integer(lines)) if *lines > 0 => {
			Some(u32::try_from(*lines).unwrap_or(u32::MAX))
		}
		_ => None,
	};
	let reference = m.capture("ref").map(|capture| capture.token.clone());
	let number = reference.as_ref().map(|_| next_listing(lower));
	let kind = ElementKind::CodeBlock {
		language,
		code,
		first_line: numbered.then_some(first_line),
		max_lines,
		number,
	};
	let attributes = Attributes {
		id: reference.as_ref().map(|r| r.content().to_string()),
		classes: Vec::new(),
	};
	lower.builder.open_with(kind, m.token.clone(), attributes);
	lower.capture(m, "title");
	let element = lower.builder.close();
	if let (Some(reference), Some(number)) = (reference, number) {
		let title = match m.capture("title").map(|capture| &capture.value) {
			Some(Value::Nodes(nodes)) => plain_text(nodes),
			_ => String::new(),
		};
		let export = listing_export(reference.content(), element, number, title);
		lower.export(export, reference);
	}
}

/// The number of the last listing of a unit.
#[derive(Default)]
struct Listings(u32);

/// Takes the next number of the unit's listings, in the order they are written, by markup or
/// by scripts.
pub fn next_listing(lower: &mut Lower<'_, '_>) -> u32 {
	let listings = lower.extension(Listings::default);
	listings.0 += 1;
	listings.0
}

/// The symbol of a listing: its title's text is its title.
pub fn listing_export(name: &str, element: ElementId, number: u32, title: String) -> Export {
	Export::new(name, SymbolKind::Listing, Some(element))
		.with("title", title)
		.with("number", number.to_string())
}

/// Warns about a language syntect does not know: the code is shown plain.
pub(crate) fn check_language(lower: &mut crate::Lower<'_, '_>, language: &nml_source::Token) {
	if !nml_render::is_known_language(language.content()) {
		let kind = DiagnosticKind::UnknownCodeLanguage {
			language: language.content().into(),
		};
		let message = DiagnosticMessage::new(language.clone());
		lower.diagnostics.push(Diagnostic::new(kind, message));
	}
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::ElementKind;
	use nml_document::SymbolKind;

	use crate::tests::check;
	use crate::tests::compile_text;

	fn block(text: &str) -> (ElementKind, String) {
		let compiled = compile_text(text);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		let (id, element) = document
			.descendants(document.root())
			.find(|(_, element)| matches!(element.kind, ElementKind::CodeBlock { .. }))
			.unwrap();
		(element.kind.clone(), document.text(id))
	}

	fn code(language: Option<&str>, code: &str, first_line: Option<i64>) -> ElementKind {
		ElementKind::CodeBlock {
			language: language.map(String::from),
			code: code.into(),
			first_line,
			max_lines: None,
			number: None,
		}
	}

	#[test]
	fn code_blocks() {
		check(
			"a\n```rust Counting **chars**\nfn f() {}\n```\nb",
			r#"document[paragraph["a"] code_block["Counting " Bold["chars"]] paragraph["b"]]"#,
		);
		assert_eq!(
			block("```rust Title\nfn f() {}\n```"),
			(code(Some("rust"), "fn f() {}", Some(1)), "Title".into())
		);
		assert_eq!(
			block("```\n*not* :: markup\n\n\\x\n```"),
			(code(None, "*not* :: markup\n\n\\x", Some(1)), String::new())
		);
		assert_eq!(block("```\n```"), (code(None, "", Some(1)), String::new()));
		assert_eq!(
			block("``` Only a title\nx\n```"),
			(code(None, "x", Some(1)), "Only a title".into())
		);
	}

	#[test]
	fn longer_fences_hold_shorter_ones() {
		assert_eq!(
			block("````md\n```\ninner\n```\n````"),
			(code(Some("md"), "```\ninner\n```", Some(1)), String::new())
		);
	}

	#[test]
	fn options() {
		assert_eq!(
			block("```rs[line_offset=10]\nx\n```"),
			(code(Some("rs"), "x", Some(10)), String::new())
		);
		assert_eq!(
			block("```rs[numbers=false] T\nx\n```"),
			(code(Some("rs"), "x", None), "T".into())
		);
		let compiled = compile_text("```rs[lines=3]\nx\n```");
		assert_diagnostics!(compiled.diagnostics, [1005]);
		// `max_lines` limits the lines shown; 0 is no limit.
		let (kind, _) = block("```rs[max_lines=20]\nx\n```");
		assert!(matches!(
			kind,
			ElementKind::CodeBlock {
				max_lines: Some(20),
				..
			}
		));
		let (kind, _) = block("```rs[max_lines=0]\nx\n```");
		assert!(matches!(
			kind,
			ElementKind::CodeBlock {
				max_lines: None,
				..
			}
		));
		let compiled = compile_text("```rs[max_lines=-2]\nx\n```");
		assert_diagnostics!(compiled.diagnostics, [1007 @ 16..18]);
	}

	#[test]
	fn errors() {
		// An unknown language is a warning: the code is shown plain.
		let compiled = compile_text("```nope\nx\n```");
		assert_diagnostics!(compiled.diagnostics, [2012 @ 3..7]);

		let compiled = compile_text("```rust\nx\n");
		assert_diagnostics!(compiled.diagnostics, [1001]);
	}

	#[test]
	fn listings() {
		let compiled = compile_text(
			"```rust #main The *entry* point\nfn main() {}\n```\n\
			```rust Not a listing\nx\n```\n\
			```rs[numbers=false] #helper\ny\n```\n\
			:figure #f F\nText.\n",
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		assert_eq!(
			crate::tests::show(document, document.root()),
			r#"document[code_block#main["The " Italic["entry"] " point"] code_block["Not a listing"] code_block#helper[] figure#f[paragraph["Text."] caption["F"]]]"#
		);
		let numbers: Vec<_> = (document.descendants(document.root()))
			.filter_map(|(_, element)| match element.kind {
				ElementKind::CodeBlock { number, .. } => Some(number),
				_ => None,
			})
			.collect();
		assert_eq!(numbers, [Some(1), None, Some(2)]);
		// Listings are numbered apart from figures.
		let exports: Vec<_> = (compiled.unit.exports.iter())
			.map(|e| {
				(
					e.name.as_str(),
					e.kind,
					e.property("number").unwrap(),
					e.property("title").unwrap(),
				)
			})
			.collect();
		assert_eq!(
			exports,
			[
				("main", SymbolKind::Listing, "1", "The entry point"),
				("helper", SymbolKind::Listing, "2", ""),
				("f", SymbolKind::Figure, "1", "F"),
			]
		);
		// The options still apply.
		assert!(matches!(
			block("```rs[numbers=false] #x T\ny\n```").0,
			ElementKind::CodeBlock {
				first_line: None,
				number: Some(1),
				..
			}
		));
	}
}

#[cfg(test)]
mod listing_tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::Dependency;
	use nml_document::ElementKind;

	use crate::tests::compile_with;

	const MAIN: &str = "use std::io;\n\nfn main() {\n    // nml:begin body\n    let x = 1;\n    if x > 0 {\n        println!(\"{x}\");\n    }\n    // nml:end body\n}\n";

	fn listing(text: &str) -> (Option<String>, String, Option<i64>) {
		let compiled = compile_with(text, &[("src/main.rs", MAIN)]);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		(document.descendants(document.root()))
			.find_map(|(_, element)| match &element.kind {
				ElementKind::CodeBlock {
					language,
					code,
					first_line,
					..
				} => Some((language.clone(), code.clone(), *first_line)),
				_ => None,
			})
			.unwrap()
	}

	#[test]
	fn lines_and_regions() {
		assert_eq!(
			listing(":listing[file=src/main.rs, lines=3-3]\n"),
			(Some("rs".into()), "fn main() {".into(), Some(3))
		);
		assert_eq!(
			listing(":listing[file=src/main.rs, lines=9-]\n").1,
			"    // nml:end body\n}"
		);
		assert_eq!(
			listing(":listing[file=/src/main.rs, lines=-1]\n").1,
			"use std::io;"
		);
		// A region, without its markers and the indentation its lines share.
		assert_eq!(
			listing(":listing[file=src/main.rs, region=body, numbers=false, language=rust]\n"),
			(
				Some("rust".into()),
				"let x = 1;\nif x > 0 {\n    println!(\"{x}\");\n}".into(),
				None
			)
		);
		let (_, all, first) = listing(":listing[file=src/main.rs]\n");
		assert_eq!(all, MAIN.trim_end());
		assert_eq!(first, Some(1));
	}

	#[test]
	fn named_and_dependent() {
		let compiled = compile_with(
			":listing[file=src/main.rs, lines=3] #entry The *entry* point\n\n[[entry]]\n",
			&[("src/main.rs", MAIN)],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let export = &compiled.unit.exports[0];
		assert_eq!(export.name, "entry");
		assert_eq!(export.property("title"), Some("The entry point"));
		assert_eq!(export.property("number"), Some("1"));
		assert_eq!(
			compiled.unit.dependencies,
			[Dependency::File {
				path: "src/main.rs".into(),
				hash: blake3::hash(MAIN.as_bytes()).to_hex().to_string(),
			}]
		);
	}

	#[test]
	fn errors() {
		let errors = |text: &str| compile_with(text, &[("src/main.rs", MAIN)]).diagnostics;
		assert_diagnostics!(errors(":listing[file=nope.rs]\n"), [3008 @ 8..22]);
		assert_diagnostics!(errors(":listing[file=src/main.rs, lines=8-30]\n"), [2032]);
		assert_diagnostics!(errors(":listing[file=src/main.rs, lines=x]\n"), [2032]);
		assert_diagnostics!(errors(":listing[file=src/main.rs, region=nope]\n"), [2033]);
		assert_diagnostics!(
			errors(":listing[file=src/main.rs, region=body, lines=1]\n"),
			[2034]
		);
		// `file` is required.
		assert_diagnostics!(errors(":listing[lines=1]\n"), [1008]);
	}
}
