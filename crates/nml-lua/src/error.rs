use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_source::Source;
use nml_source::Token;

/// The chunks of a unit's Lua, by name, with the virtual source of their code.
#[derive(Debug, Default)]
pub(crate) struct Chunks {
	chunks: Vec<(String, Arc<Source>)>,
}

impl Chunks {
	pub fn add(&mut self, name: String, source: Arc<Source>) {
		self.chunks.push((name, source));
	}

	/// The chunk and line a `name:line:` prefix of `text` refers to, and the text after it.
	fn locate<'t>(&self, text: &'t str) -> Option<(Token, &'t str)> {
		self.chunks.iter().find_map(|(name, source)| {
			let rest = text.strip_prefix(name.as_str())?.strip_prefix(':')?;
			let (line, rest) = rest.split_once(':')?;
			let line: usize = line.parse().ok()?;
			let range = source.line_range(line.checked_sub(1)?)?;
			// The line without its indentation.
			let content = &source.content()[range.clone()];
			let start = range.start + (content.len() - content.trim_start().len());
			Some((
				Token::new(source.clone(), start..range.end.max(start)),
				rest.trim_start(),
			))
		})
	}

	/// A Lua error as a diagnostic: Lua's own message, at the line it names, with the
	/// traceback's frames as secondary locations. Positions inside chunks map to the `.nml`
	/// file through their virtual source. Errors that name no known chunk are reported at
	/// `fallback`.
	pub fn diagnostic(&self, error: &mlua::Error, fallback: Token) -> Diagnostic {
		let text = match error {
			mlua::Error::RuntimeError(text) => text.clone(),
			mlua::Error::SyntaxError { message, .. } => message.clone(),
			mlua::Error::CallbackError { traceback, cause } => format!("{cause}\n{traceback}"),
			error => error.to_string(),
		};
		let (message, traceback) = match text.split_once("\nstack traceback:") {
			Some((message, traceback)) => (message, traceback),
			None => (text.as_str(), ""),
		};
		let (location, message) = match self.locate(message) {
			Some((token, rest)) => (token, rest.to_string()),
			None => (fallback, message.to_string()),
		};
		let mut diagnostic = Diagnostic::new(
			DiagnosticKind::LuaError { message },
			DiagnosticMessage::new(location.clone()),
		);
		for frame in traceback.lines() {
			if let Some((token, rest)) = self.locate(frame.trim()) {
				if token != location {
					diagnostic = diagnostic.with(DiagnosticMessage::text(token, rest));
				}
			}
		}
		diagnostic
	}
}

#[cfg(test)]
mod tests {
	use nml_source::Url;
	use nml_source::VirtualSourceBuilder;

	use super::*;
	use crate::Limits;
	use crate::Runtime;

	/// A file with two Lua chunks: a block at lines 2-4, and a role at line 7.
	fn setup() -> (Arc<Source>, Chunks, Runtime) {
		let text = ":lua\n  function f()\n    error('boom')\n  end\ntext\n\nx :lua`f()` y\n";
		let file = Arc::new(Source::new(
			Url::parse("file:///a.nml").unwrap(),
			text.into(),
			0,
		));
		let mut chunks = Chunks::default();
		let runtime = Runtime::new(Limits::default()).unwrap();
		let mut add = |name: &str, range: std::ops::Range<usize>| {
			let url = Url::parse(&format!("virtual:///a.nml/lua/{}", range.start)).unwrap();
			let mut builder = VirtualSourceBuilder::new(file.clone(), url, range.start);
			builder.copy(range);
			let source = Arc::new(builder.build());
			chunks.add(name.into(), source.clone());
			source
		};
		let block = add("a.nml@5", 5..44);
		runtime.run(block.content(), "a.nml@5").unwrap();
		add("a.nml@57", 57..60);
		(file, chunks, runtime)
	}

	#[test]
	fn errors_map_to_the_file() {
		let (file, chunks, runtime) = setup();
		// Not a tail call, so the calling frame stays in the traceback.
		let error = runtime.run("local _ = f()", "a.nml@57").unwrap_err();
		let diagnostic = chunks.diagnostic(&error, Token::new(file.clone(), 0..1));

		assert_eq!(
			diagnostic.kind,
			DiagnosticKind::LuaError {
				message: "boom".into()
			}
		);
		let primary = diagnostic.primary().location.root();
		assert_eq!(primary.content(), "error('boom')");
		assert_eq!(primary.range(), 24..37);
		// The frames: `f` where it raised (the primary, not repeated), and the role.
		let frames: Vec<_> = diagnostic.messages()[1..]
			.iter()
			.map(|m| (m.location.root().content().to_string(), m.content.clone()))
			.collect();
		assert_eq!(
			frames,
			[(
				"f()".to_string(),
				Some(nml_diagnostics::MessageContent::Text(
					"in main chunk".into()
				))
			)]
		);
	}

	#[test]
	fn syntax_errors_and_unknown_chunks() {
		let (file, chunks, runtime) = setup();
		let error = runtime.run("1 +", "a.nml@57").unwrap_err();
		let diagnostic = chunks.diagnostic(&error, Token::new(file.clone(), 0..1));
		assert_eq!(diagnostic.primary().location.root().range(), 57..60);
		let DiagnosticKind::LuaError { message } = &diagnostic.kind else {
			panic!()
		};
		assert!(message.starts_with("unexpected symbol"), "{message}");

		// An error raised with a table names no chunk: it is reported at the fallback.
		let error = runtime.run("error({})", "elsewhere").unwrap_err();
		let diagnostic = chunks.diagnostic(&error, Token::new(file, 0..4));
		assert_eq!(diagnostic.primary().location.range(), 0..4);
	}
}
