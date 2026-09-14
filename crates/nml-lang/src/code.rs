use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::ElementKind;
use nml_grammar::Atom;
use nml_grammar::OptionKind;
use nml_grammar::OptionSchema;
use nml_grammar::OptionSpec;
use nml_grammar::OptionValue;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::Value;

use crate::Language;

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
		numbered), `max_lines` (how many lines are shown before the rest scrolls). A longer fence lets the code hold ```` ``` ````.",
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
		let kind = ElementKind::CodeBlock {
			language: language.map(|language| language.content().to_string()),
			code: code.to_string(),
			first_line: numbered.then_some(first_line),
			max_lines,
		};
		lower.builder.open(kind, m.token.clone());
		lower.capture(m, "title");
		lower.builder.close();
	})?;
	Ok(())
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
}
