use nml_source::Token;

use crate::Argument;
use crate::DiagnosticKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DiagnosticSeverity {
	Hint,
	Warning,
	Error,
}

/// The text of a message or fix.
#[derive(Debug, Clone, PartialEq)]
pub enum MessageContent {
	/// An attribute of the kind's Fluent message (`.opened`, `.ended`, …).
	///
	/// It is formatted with the kind's arguments plus `args`.
	Fluent {
		attribute: &'static str,
		args: Vec<(&'static str, Argument)>,
	},
	/// Text shown as is, without localization (Lua diagnostics).
	Text(String),
}

/// A location in a diagnostic, with an optional label.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticMessage {
	pub location: Token,
	pub content: Option<MessageContent>,
}

impl DiagnosticMessage {
	/// A location without a label.
	pub fn new(location: Token) -> Self {
		Self {
			location,
			content: None,
		}
	}

	/// A location labelled with an attribute of the kind's Fluent message.
	pub fn fluent(location: Token, attribute: &'static str) -> Self {
		Self {
			location,
			content: Some(MessageContent::Fluent {
				attribute,
				args: Vec::new(),
			}),
		}
	}

	/// A location labelled with unlocalized text.
	pub fn text(location: Token, text: impl Into<String>) -> Self {
		Self {
			location,
			content: Some(MessageContent::Text(text.into())),
		}
	}

	/// Adds a Fluent argument to the label.
	///
	/// # Panics
	///
	/// If the label is not a Fluent attribute.
	pub fn arg(mut self, name: &'static str, value: impl Into<Argument>) -> Self {
		match &mut self.content {
			Some(MessageContent::Fluent { args, .. }) => args.push((name, value.into())),
			_ => panic!("`{name}` is a Fluent argument, but the label is not a Fluent attribute"),
		}
		self
	}
}

/// A text edit, part of a [`Fix`].
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
	pub location: Token,
	pub replacement: String,
}

/// A suggested change that resolves a diagnostic: an LSP code action.
#[derive(Debug, Clone, PartialEq)]
pub struct Fix {
	pub title: MessageContent,
	pub edits: Vec<Edit>,
}

/// A problem found in a document.
///
/// A diagnostic has at least one message. The first one is the primary location: the range a
/// diagnostic is shown at. The others are secondary locations, e.g. where an unclosed
/// construct was opened.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
	pub severity: DiagnosticSeverity,
	pub kind: DiagnosticKind,
	messages: Vec<DiagnosticMessage>,
	pub fixes: Vec<Fix>,
}

impl Diagnostic {
	/// A diagnostic at `primary`, with the kind's default severity.
	pub fn new(kind: DiagnosticKind, primary: DiagnosticMessage) -> Self {
		Self {
			severity: kind.default_severity(),
			kind,
			messages: vec![primary],
			fixes: Vec::new(),
		}
	}

	/// Adds a secondary location.
	pub fn with(mut self, message: DiagnosticMessage) -> Self {
		self.messages.push(message);
		self
	}

	pub fn with_fix(mut self, fix: Fix) -> Self {
		self.fixes.push(fix);
		self
	}

	pub fn with_severity(mut self, severity: DiagnosticSeverity) -> Self {
		self.severity = severity;
		self
	}

	pub fn primary(&self) -> &DiagnosticMessage {
		&self.messages[0]
	}

	/// All messages, the primary one first.
	pub fn messages(&self) -> &[DiagnosticMessage] {
		&self.messages
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use nml_source::Source;
	use nml_source::Url;

	use super::*;

	fn source() -> Arc<Source> {
		Arc::new(Source::new(
			Url::parse("file:///test.nml").unwrap(),
			"some **bold\n\nnext".into(),
			0,
		))
	}

	#[test]
	fn unclosed_bold() {
		let source = source();
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

		assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
		assert_eq!(diagnostic.primary().location.content(), "**");
		assert_eq!(diagnostic.messages().len(), 2);
		assert_eq!(
			diagnostic.primary().content,
			Some(MessageContent::Fluent {
				attribute: "opened",
				args: vec![("style", Argument::from("bold"))],
			})
		);
	}

	#[test]
	fn severity_override() {
		let diagnostic = Diagnostic::new(
			DiagnosticKind::LuaError {
				message: "boom".into(),
			},
			DiagnosticMessage::text(Token::new(source(), 0..4), "raised here"),
		)
		.with_severity(DiagnosticSeverity::Warning);
		assert_eq!(diagnostic.severity, DiagnosticSeverity::Warning);
	}

	#[test]
	#[should_panic(expected = "not a Fluent attribute")]
	fn arg_on_text_label_panics() {
		let _ = DiagnosticMessage::text(Token::new(source(), 0..4), "text").arg("x", 1);
	}
}
