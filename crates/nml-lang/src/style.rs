use nml_document::ElementKind;
use nml_document::Style;
use nml_grammar::Atom;
use nml_grammar::Rule;
use nml_grammar::RuleError;

use crate::Language;

/// `**bold**`, `*italic*`, `__underline__`, `==highlight==` and `~~strikethrough~~`.
fn capitalize(name: &str) -> String {
	name[..1].to_uppercase() + &name[1..]
}

pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	for (name, marker, style) in [
		("bold", "**", Style::Bold),
		("italic", "*", Style::Italic),
		("underline", "__", Style::Underline),
		("highlight", "==", Style::Highlight),
		("strikethrough", "~~", Style::Strikethrough),
	] {
		let rule = Rule::new(
			name,
			vec![
				Atom::capture("open", Atom::literal(marker)).with_class("operator"),
				Atom::capture("body", Atom::InlineContent),
				Atom::capture("close", Atom::literal(marker)).with_class("operator"),
			],
		)
		.with_doc(format!(
			"**{} text**: `{marker}text{marker}`.\n\nStyles nest: `***both***` is bold and \
			italic. A marker is text when escaped: `\\{marker}`.",
			capitalize(name)
		))
		.with_snippet(format!("{marker}${{1:text}}{marker}"));
		language.add(rule, move |lower, m| {
			lower
				.builder
				.open(ElementKind::Style(style), m.token.clone());
			lower.capture(m, "body");
			lower.builder.close();
		})?;
	}
	Ok(())
}
