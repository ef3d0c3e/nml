use nml_document::ElementKind;
use nml_grammar::Atom;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::Value;

use crate::Language;

/// Code spans `` `code` `` and links `[text](url)`.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	// As in Markdown, a longer fence lets the code hold backticks: ``` `` a ` b `` ```.
	let code = Rule::new(
		"code_span",
		vec![
			Atom::capture("open", Atom::run('`', 1)).with_class("operator"),
			Atom::capture("code", Atom::Text).with_class("string"),
			Atom::capture("close", Atom::same("open")).with_class("operator"),
		],
	)
	.with_doc(
		"**Inline code**: ``` `code` ```, shown as written.\n\n\
		A longer fence lets the code hold backticks: ``` `` a ` b `` ```.",
	)
	.with_snippet("`${1:code}`");
	language.add(code, |lower, m| {
		let code = code_text(m.capture("code").unwrap().token.content());
		let kind = ElementKind::Code {
			code,
			language: None,
		};
		lower.builder.leaf(kind, m.token.clone());
	})?;

	let role = Rule::new(
		"code_role",
		vec![
			Atom::capture("command", Atom::literal(":code[")).with_class("keyword"),
			Atom::capture("language", Atom::Word).with_class("type"),
			Atom::capture("bracket", Atom::literal("]")).with_class("keyword"),
			Atom::capture("open", Atom::run('`', 1)).with_class("operator"),
			Atom::capture("code", Atom::Text).with_class("string"),
			Atom::capture("close", Atom::same("open")).with_class("operator"),
		],
	)
	.with_doc(
		"**Inline code in a language**, highlighted: ``` :code[rust]`fn main()` ```.\n\n\
		The language is named as for code blocks: `rust` or `rs`.",
	)
	.with_snippet(":code[${1:language}]`${2:code}`");
	language.add(role, |lower, m| {
		let language = m.capture("language").unwrap().token.clone();
		crate::code::check_language(lower, &language);
		let kind = ElementKind::Code {
			code: code_text(m.capture("code").unwrap().token.content()),
			language: Some(language.content().to_string()),
		};
		lower.builder.leaf(kind, m.token.clone());
	})?;

	let link = Rule::new(
		"link",
		vec![
			Atom::capture("open", Atom::literal("[")).with_class("operator"),
			Atom::capture("text", Atom::InlineContent),
			Atom::capture("middle", Atom::literal("](")).with_class("operator"),
			Atom::capture("url", Atom::Text).with_class("string"),
			Atom::capture("close", Atom::literal(")")).with_class("operator"),
		],
	)
	.with_doc(
		"**Link** to a URL: `[text](https://example.org)`.\n\n\
		Without a text, `[](url)` shows the URL. To link to a heading, figure or page of the \
		workspace, use a reference: `[[name]]`.",
	)
	.with_snippet("[${1:text}](${2:url})");
	language.add(link, |lower, m| {
		let url = m.capture("url").unwrap().token.content().trim().to_string();
		let text = match m.capture("text").map(|capture| &capture.value) {
			Some(Value::Nodes(nodes)) if !nodes.is_empty() => Some(nodes),
			_ => None,
		};
		let kind = ElementKind::Link { url: url.clone() };
		lower.builder.open(kind, m.token.clone());
		match text {
			Some(nodes) => lower.nodes(nodes),
			// Without a text, the link shows its URL.
			None => {
				lower.builder.leaf(ElementKind::Text(url), m.token.clone());
			}
		}
		lower.builder.close();
	})?;
	Ok(())
}

/// The text of a code span, as in Markdown: line breaks become spaces, and one space is
/// stripped from each end if both have one, so that `` `` `a` `` `` holds `` `a` ``.
fn code_text(code: &str) -> String {
	let code = code.replace("\r\n", " ").replace('\n', " ");
	let padded = code.len() >= 2 && code.starts_with(' ') && code.ends_with(' ');
	if padded && !code.chars().all(|c| c == ' ') {
		code[1..code.len() - 1].to_string()
	} else {
		code
	}
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::ElementKind;

	use crate::tests::check;

	fn kinds(text: &str) -> Vec<ElementKind> {
		let compiled = check_ok(text);
		let document = &compiled.unit.document;
		document
			.descendants(document.root())
			.map(|(_, element)| element.kind.clone())
			.filter(|kind| matches!(kind, ElementKind::Code { .. } | ElementKind::Link { .. }))
			.collect()
	}

	fn check_ok(text: &str) -> crate::Compiled {
		let compiled = crate::tests::compile_text(text);
		assert_diagnostics!(compiled.diagnostics, []);
		compiled
	}

	#[test]
	fn code_spans() {
		check("a `b *c*` d", r#"document[paragraph["a " code[] " d"]]"#);
		let code = |text: &str| ElementKind::Code {
			code: text.into(),
			language: None,
		};
		assert_eq!(kinds("`b *c*`"), [code("b *c*")]);
		assert_eq!(kinds("`` a ` b ``"), [code("a ` b")]);
		assert_eq!(kinds("`` `a` ``"), [code("`a`")]);
		assert_eq!(kinds("`  `"), [code("  ")]);
		assert_eq!(kinds("` a`"), [code(" a")]);
		assert_eq!(kinds("`a\nb`"), [code("a b")]);
		// Backslashes are verbatim.
		assert_eq!(kinds(r"`a\*`"), [code(r"a\*")]);

		let compiled = crate::tests::compile_text("`a");
		assert_diagnostics!(compiled.diagnostics, [1001 @ 0..1]);
	}

	#[test]
	fn code_in_a_language() {
		check(
			"a :code[rs]`fn x()` b",
			r#"document[paragraph["a " code[] " b"]]"#,
		);
		let code = |text: &str, language: &str| ElementKind::Code {
			code: text.into(),
			language: Some(language.into()),
		};
		assert_eq!(kinds(":code[rs]`fn x()`"), [code("fn x()", "rs")]);
		assert_eq!(kinds(":code[rust]`` a ` b ``"), [code("a ` b", "rust")]);
		// An unknown language is a warning, and the code is kept.
		let compiled = crate::tests::compile_text(":code[nope]`x`");
		assert_diagnostics!(compiled.diagnostics, [2012 @ 6..10]);
		// Without a language, `:code` is text.
		check(":code`x`", r#"document[paragraph[":code" code[]]]"#);
	}

	#[test]
	fn links() {
		check(
			"See [the **docs**](https://nml.example/docs) or [](a.html).",
			r#"document[paragraph["See " link["the " Bold["docs"]] " or " link["a.html"] "."]]"#,
		);
		let link = |url: &str| ElementKind::Link { url: url.into() };
		assert_eq!(kinds("[a]( https://x.y/z )"), [link("https://x.y/z")]);
		// A reference opens with `[[`, not a link.
		check(
			"[[x|y]] \\[z]",
			r#"document[paragraph[reference["y"] " [z]"]]"#,
		);

		let compiled = crate::tests::compile_text("[a] b");
		assert_diagnostics!(compiled.diagnostics, [1001]);
	}
}
