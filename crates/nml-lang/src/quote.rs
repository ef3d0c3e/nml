use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::CalloutKind;
use nml_document::ElementKind;
use nml_grammar::Atom;
use nml_grammar::OptionKind;
use nml_grammar::OptionSchema;
use nml_grammar::OptionSpec;
use nml_grammar::OptionValue;
use nml_grammar::Prefix;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::Value;

use crate::Language;

/// Quotes `> text` and callouts `> [!kind] Title`. `> [!quote][author=…, cite=…, url=…]`
/// is a quote with its source. The lines of both start with `> `.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let body = || Atom::capture("body", Atom::Container(Prefix::Literal("> ".into())));
	let quote = Rule::new(
		"quote",
		vec![
			Atom::LineStart,
			Atom::capture("marker", Atom::literal("> ")).with_class("operator"),
			body(),
		],
	);
	language.add(quote, |lower, m| {
		let quote = ElementKind::Quote {
			author: None,
			cite: None,
			url: None,
		};
		lower.builder.open(quote, m.token.clone());
		lower.capture(m, "body");
		lower.builder.close();
	})?;

	let text = |name: &str, doc: &str| {
		OptionSpec::new(name, OptionKind::Text)
			.with_default(OptionValue::Text(String::new()))
			.with_doc(doc)
	};
	let schema = OptionSchema::new([
		text("author", "Who said or wrote the quotation."),
		text("cite", "The title of the quoted work."),
		text("url", "Where the quotation comes from."),
	]);
	let callout = Rule::new(
		"callout",
		vec![
			Atom::LineStart,
			Atom::capture("open", Atom::literal("> [!")).with_class("operator"),
			Atom::capture("kind", Atom::Word).with_class("keyword"),
			Atom::capture("close", Atom::literal("]")).with_class("operator"),
			Atom::optional(vec![Atom::capture(
				"options",
				Atom::Options(Arc::new(schema)),
			)]),
			Atom::optional(vec![
				Atom::literal(" "),
				Atom::capture("title", Atom::InlineContent),
			]),
			Atom::LineEnd,
			body(),
		],
	);
	language.add(callout, |lower, m| {
		let kind_token = m.capture("kind").unwrap().token.clone();
		let options = match m.capture("options").map(|capture| &capture.value) {
			Some(Value::Options(options)) => Some(options),
			_ => None,
		};
		let element = if kind_token.content().eq_ignore_ascii_case("quote") {
			let option = |name| match options.and_then(|options| options.get(name)) {
				Some(OptionValue::Text(text)) if !text.is_empty() => Some(text.clone()),
				_ => None,
			};
			ElementKind::Quote {
				author: option("author"),
				cite: option("cite"),
				url: option("url"),
			}
		} else if let Some(kind) = CalloutKind::from_name(kind_token.content()) {
			// The options are the source of a quote.
			for entry in options.iter().flat_map(|options| &options.entries) {
				let kind = DiagnosticKind::UnknownOption {
					name: entry.key.content().into(),
				};
				let message = DiagnosticMessage::new(entry.key.clone());
				lower.diagnostics.push(Diagnostic::new(kind, message));
			}
			ElementKind::Callout(kind)
		} else {
			let kinds: Vec<_> = CalloutKind::ALL.iter().map(|kind| kind.name()).collect();
			let kind = DiagnosticKind::UnknownCallout {
				kind: kind_token.content().into(),
				kinds: format!("quote, {}", kinds.join(", ")),
			};
			let message = DiagnosticMessage::new(kind_token);
			lower.diagnostics.push(Diagnostic::new(kind, message));
			return;
		};
		lower.builder.open(element, m.token.clone());
		if let Some(title) = m.capture("title") {
			lower
				.builder
				.open(ElementKind::Caption, title.token.clone());
			lower.capture(m, "title");
			lower.builder.close();
		}
		lower.capture(m, "body");
		lower.builder.close();
	})?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::CalloutKind;
	use nml_document::ElementKind;

	use crate::tests::check;
	use crate::tests::compile_text;

	fn first(text: &str) -> ElementKind {
		let compiled = compile_text(text);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		let first = document.get(document.root()).children()[0];
		document.get(first).kind.clone()
	}

	#[test]
	fn quotes() {
		check(
			"> a\n> **b**\n>\n> - c\n\n> d\nafter",
			r#"document[quote[paragraph["a\n" Bold["b"]] list[list_item[paragraph["c"]]]] quote[paragraph["d"]] paragraph["after"]]"#,
		);
		check(
			"> > nested\n> back",
			r#"document[quote[quote[paragraph["nested"]] paragraph["back"]]]"#,
		);
	}

	#[test]
	fn callouts() {
		check(
			"> [!note]\n> Read *this*.\n\n> [!WARNING] Be **careful**\n> x",
			r#"document[callout[paragraph["Read " Italic["this"] "."]] callout[caption["Be " Bold["careful"]] paragraph["x"]]]"#,
		);
		assert_eq!(
			first("> [!Todo]\n> x"),
			ElementKind::Callout(CalloutKind::Todo)
		);
		// A callout can be empty.
		assert_eq!(first("> [!tip]"), ElementKind::Callout(CalloutKind::Tip));
	}

	#[test]
	fn quotes_with_their_source() {
		assert_eq!(
			first("> [!quote][author=Ada Lovelace, cite=Notes, url=\"https://x.y\"]\n> x"),
			ElementKind::Quote {
				author: Some("Ada Lovelace".into()),
				cite: Some("Notes".into()),
				url: Some("https://x.y".into()),
			}
		);
		assert_eq!(
			first("> [!quote]\n> x"),
			ElementKind::Quote {
				author: None,
				cite: None,
				url: None,
			}
		);
	}

	#[test]
	fn errors() {
		let compiled = compile_text("> [!nope]\n> x");
		assert_diagnostics!(compiled.diagnostics, [2014 @ 4..8]);
		let compiled = compile_text("> [!note][author=x]\n> x");
		assert_diagnostics!(compiled.diagnostics, [1005 @ 10..16]);
		// `[!` after `> ` always opens a callout.
		let compiled = compile_text("> [!x y]");
		assert_diagnostics!(compiled.diagnostics, [1001]);
		let compiled = check("> \\[!x y]", r#"document[quote[paragraph["[!x y]"]]]"#);
		assert_diagnostics!(compiled.diagnostics, []);
	}
}
