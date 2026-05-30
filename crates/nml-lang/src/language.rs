use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_document::Dependency;
use nml_document::DocumentBuilder;
use nml_document::ElementKind;
use nml_document::Export;
use nml_document::Unit;
use nml_document::Use;
use nml_grammar::Node;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::RuleId;
use nml_grammar::RuleMatch;
use nml_grammar::RuleSet;
use nml_grammar::Value;
use nml_source::Source;
use nml_source::Token;

/// Lowers a match of its rule into document elements.
pub type Handler = Box<dyn Fn(&mut Lower<'_, '_>, &RuleMatch) + Send + Sync>;

/// A grammar and the handlers of its rules.
pub struct Language {
	rules: RuleSet,
	handlers: HashMap<RuleId, Handler>,
}

impl fmt::Debug for Language {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Language")
			.field("rules", &self.rules)
			.finish_non_exhaustive()
	}
}

impl Language {
	/// A language without rules: everything is paragraphs of text.
	pub fn empty() -> Self {
		Self {
			rules: RuleSet::new(),
			handlers: HashMap::new(),
		}
	}

	/// Adds a rule and the handler that lowers its matches.
	pub fn add(
		&mut self,
		rule: Rule,
		handler: impl Fn(&mut Lower<'_, '_>, &RuleMatch) + Send + Sync + 'static,
	) -> Result<RuleId, RuleError> {
		let id = self.rules.add(rule)?;
		self.handlers.insert(id, Box::new(handler));
		Ok(id)
	}

	pub fn rules(&self) -> &RuleSet {
		&self.rules
	}
}

/// The result of compiling a source.
#[derive(Debug)]
pub struct Compiled {
	pub unit: Unit,
	/// Parse errors and errors found while lowering. A unit with errors is not committed to
	/// the workspace.
	pub diagnostics: Vec<Diagnostic>,
}

/// Parses a source and lowers it into a unit.
pub fn compile(language: &Language, source: Arc<Source>) -> Compiled {
	let parsed = nml_grammar::parse(&language.rules, source.clone());
	let mut lower = Lower {
		language,
		builder: DocumentBuilder::new(source),
		exports: Vec::new(),
		uses: Vec::new(),
		dependencies: Vec::new(),
		diagnostics: parsed.diagnostics,
	};
	lower.nodes(&parsed.nodes);
	let Lower {
		builder,
		exports,
		uses,
		dependencies,
		diagnostics,
		..
	} = lower;
	Compiled {
		unit: Unit {
			document: builder.finish(),
			exports,
			uses,
			dependencies,
		},
		diagnostics,
	}
}

/// The state of lowering a parse tree into a unit, given to rule handlers.
pub struct Lower<'a, 'l> {
	language: &'a Language,
	pub builder: DocumentBuilder<'l>,
	pub exports: Vec<Export>,
	pub uses: Vec<Use>,
	pub dependencies: Vec<Dependency>,
	pub diagnostics: Vec<Diagnostic>,
}

impl fmt::Debug for Lower<'_, '_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Lower")
			.field("builder", &self.builder)
			.finish_non_exhaustive()
	}
}

impl Lower<'_, '_> {
	/// Lowers nodes into the open element. Adjacent text and escapes become one text element.
	pub fn nodes(&mut self, nodes: &[Node]) {
		let mut text: Option<(Token, String)> = None;
		for node in nodes {
			match node {
				Node::Text(token) | Node::Escape(token) => {
					let content = match node {
						Node::Escape(token) => &token.content()[1..],
						_ => token.content(),
					};
					match &mut text {
						Some((start, buffer)) if start.end() == token.start() => {
							*start = Token::new(start.source().clone(), start.start()..token.end());
							buffer.push_str(content);
						}
						_ => {
							self.flush(text.take());
							text = Some((token.clone(), content.to_string()));
						}
					}
				}
				Node::Paragraph { token, children } => {
					self.flush(text.take());
					self.builder.open(ElementKind::Paragraph, token.clone());
					self.nodes(children);
					self.builder.close();
				}
				Node::Match(m) => {
					self.flush(text.take());
					if m.complete {
						let handler = &self.language.handlers[&m.rule];
						handler(self, m);
					} else {
						// The unit has an error and will not be committed; its content is
						// kept for the rest of the tree.
						for capture in &m.captures {
							if let Value::Nodes(nodes) = &capture.value {
								self.nodes(nodes);
							}
						}
					}
				}
			}
		}
		self.flush(text);
	}

	/// Lowers the nodes of a capture, if it holds any.
	pub fn capture(&mut self, m: &RuleMatch, name: &str) {
		if let Some(Value::Nodes(nodes)) = m.capture(name).map(|capture| &capture.value) {
			self.nodes(nodes);
		}
	}

	fn flush(&mut self, text: Option<(Token, String)>) {
		if let Some((token, text)) = text {
			self.builder.leaf(ElementKind::Text(text), token);
		}
	}
}
