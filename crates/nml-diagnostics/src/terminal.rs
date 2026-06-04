use std::io;
use std::sync::Arc;

use ariadne::Color;
use ariadne::Config;
use ariadne::IndexType;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use nml_source::Source;

use crate::Diagnostic;
use crate::DiagnosticSeverity;
use crate::Localizer;

/// Writes a diagnostic for a terminal: its code, title and source snippets, with fixes as
/// notes.
///
/// Locations in virtual sources are shown in the real file they come from.
pub fn render(
	diagnostic: &Diagnostic,
	localizer: &Localizer,
	color: bool,
	out: impl io::Write,
) -> io::Result<()> {
	let localized = localizer.localize(diagnostic);
	let (kind, primary_color) = match diagnostic.severity {
		DiagnosticSeverity::Error => (ReportKind::Error, Color::Red),
		DiagnosticSeverity::Warning => (ReportKind::Warning, Color::Yellow),
		DiagnosticSeverity::Hint => (ReportKind::Advice, Color::Cyan),
	};

	let mut sources: Vec<Arc<Source>> = Vec::new();
	let mut spans = Vec::new();
	for message in diagnostic.messages() {
		let root = message.location.root();
		if !sources
			.iter()
			.any(|source| Arc::ptr_eq(source, root.source()))
		{
			sources.push(root.source().clone());
		}
		spans.push((name(root.source()), root.range()));
	}

	let mut report = Report::build(kind, spans[0].clone())
		.with_config(
			Config::default()
				.with_color(color)
				.with_index_type(IndexType::Byte),
		)
		.with_code(diagnostic.kind.code())
		.with_message(&localized.title);
	for (index, (span, text)) in spans.into_iter().zip(localized.labels).enumerate() {
		let mut label = Label::new(span)
			.with_order(index as i32)
			.with_color(if index == 0 {
				primary_color
			} else {
				Color::Blue
			});
		// ariadne does not underline a label without a message: the primary location
		// repeats the title instead.
		let text = text.or_else(|| (index == 0).then(|| localized.title.clone()));
		if let Some(text) = text {
			label = label.with_message(text);
		}
		report.add_label(label);
	}
	for fix in localized.fixes {
		report.add_note(fix);
	}

	report.finish().write(
		ariadne::sources(
			sources
				.iter()
				.map(|source| (name(source), source.content().to_string())),
		),
		out,
	)
}

/// The name a source is shown with: a path for local files, the URL otherwise.
fn name(source: &Source) -> String {
	let location = source.location();
	match location.to_file_path() {
		Ok(path) if location.scheme() == "file" => path.display().to_string(),
		_ => location.to_string(),
	}
}

#[cfg(test)]
mod tests {
	use nml_source::Token;
	use nml_source::Url;
	use nml_source::VirtualSourceBuilder;

	use crate::DiagnosticKind;
	use crate::DiagnosticMessage;
	use crate::Edit;
	use crate::Fix;
	use crate::MessageContent;

	use super::*;

	fn rendered(diagnostic: &Diagnostic) -> String {
		let mut out = Vec::new();
		render(diagnostic, &Localizer::new("en-US"), false, &mut out).unwrap();
		String::from_utf8(out).unwrap()
	}

	#[test]
	fn unclosed_bold() {
		let source = Arc::new(Source::new(
			Url::parse("file:///doc/a.nml").unwrap(),
			"some **bold\n\nnext\n".into(),
			0,
		));
		let diagnostic = Diagnostic::new(
			DiagnosticKind::ExpectedToken { token: "**".into() },
			DiagnosticMessage::fluent(Token::new(source.clone(), 5..7), "opened")
				.arg("style", "bold"),
		)
		.with(DiagnosticMessage::fluent(
			Token::new(source.clone(), 11..12),
			"ended",
		))
		.with_fix(Fix {
			title: MessageContent::Fluent {
				attribute: "fix-escape",
				args: Vec::new(),
			},
			edits: vec![Edit {
				location: Token::new(source, 5..5),
				replacement: "\\".into(),
			}],
		});

		let output = rendered(&diagnostic);
		assert!(
			output.starts_with("[NML1001] Error: expected `**`"),
			"{output}"
		);
		assert!(output.contains("/doc/a.nml:1:6"), "{output}");
		assert!(output.contains("bold opened here"), "{output}");
		assert!(output.contains("ended here without `**`"), "{output}");
		assert!(
			output.contains("Note: Escape the opening delimiter"),
			"{output}"
		);
	}

	#[test]
	fn virtual_locations_show_the_real_file() {
		let parent = Arc::new(Source::new(
			Url::parse("file:///doc/a.nml").unwrap(),
			"intro\n:lua\n```\nerror('x')\n```\n".into(),
			0,
		));
		let mut builder =
			VirtualSourceBuilder::new(parent, Url::parse("virtual:///a.nml/lua/0").unwrap(), 15);
		builder.copy(15..26);
		let lua = Arc::new(builder.build());

		let diagnostic = Diagnostic::new(
			DiagnosticKind::LuaError {
				message: "x".into(),
			},
			DiagnosticMessage::text(Token::new(lua, 0..5), "raised here"),
		)
		.with_severity(DiagnosticSeverity::Warning);

		let output = rendered(&diagnostic);
		assert!(output.starts_with("[NML4001] Warning: x"), "{output}");
		assert!(output.contains("/doc/a.nml:4:1"), "{output}");
		assert!(output.contains("error('x')"), "{output}");
		assert!(!output.contains("virtual:"), "{output}");
	}
}
