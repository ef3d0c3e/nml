use std::any::Any;
use std::any::TypeId;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
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

use crate::Resolver;

/// Lowers a match of its rule into document elements.
pub type Handler = Arc<dyn Fn(&mut Lower<'_, '_>, &RuleMatch) + Send + Sync>;

/// Lowers a run of consecutive matches of grouped rules with the same group key.
pub type GroupHandler = Arc<dyn Fn(&mut Lower<'_, '_>, &[&RuleMatch]) + Send + Sync>;

/// The group key of a match: consecutive matches with the same key are lowered together.
pub type GroupKey = Arc<dyn Fn(&RuleMatch) -> String + Send + Sync>;

/// Called for every module a unit imports, once it is resolved.
pub type ImportHook = Arc<dyn Fn(&mut Lower<'_, '_>, &Import) + Send + Sync>;

/// A resolved import.
#[derive(Debug, Clone)]
pub struct Import {
	/// The module path, `std.toc`.
	pub module: String,
	/// What the module's exports are visible as.
	pub namespace: String,
	/// The module's file, in the workspace.
	pub path: String,
	pub source: Arc<Source>,
	/// Where the import is written.
	pub token: Token,
}

/// A grammar and the handlers of its rules.
#[derive(Clone)]
pub struct Language {
	rules: RuleSet,
	handlers: HashMap<RuleId, Handler>,
	/// Rules whose consecutive matches are lowered together, such as list items.
	groups: HashMap<RuleId, (GroupKey, GroupHandler)>,
	/// Rules allowed in the prologue: they do not end it.
	prologue: HashSet<RuleId>,
	import_hooks: Vec<ImportHook>,
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
			groups: HashMap::new(),
			prologue: HashSet::new(),
			import_hooks: Vec::new(),
		}
	}

	/// Adds a rule and the handler that lowers its matches.
	pub fn add(
		&mut self,
		rule: Rule,
		handler: impl Fn(&mut Lower<'_, '_>, &RuleMatch) + Send + Sync + 'static,
	) -> Result<RuleId, RuleError> {
		let id = self.rules.add(rule)?;
		self.handlers.insert(id, Arc::new(handler));
		Ok(id)
	}

	/// Adds a rule whose consecutive matches are lowered together when `key` gives them the
	/// same key, like the items of a list. Matches of different grouped rules join a run if
	/// their keys are equal; the handler of the run's first rule lowers it.
	pub fn add_grouped(
		&mut self,
		rule: Rule,
		key: impl Fn(&RuleMatch) -> String + Send + Sync + 'static,
		handler: impl Fn(&mut Lower<'_, '_>, &[&RuleMatch]) + Send + Sync + 'static,
	) -> Result<RuleId, RuleError> {
		let handler: GroupHandler = Arc::new(handler);
		let single = handler.clone();
		let id = self.add(rule, move |lower, m| single(lower, &[m]))?;
		self.groups.insert(id, (Arc::new(key), handler));
		Ok(id)
	}

	/// Adds a rule that may appear in the prologue: the commands before the first content
	/// block, such as `:import` and `:set`.
	pub fn add_prologue(
		&mut self,
		rule: Rule,
		handler: impl Fn(&mut Lower<'_, '_>, &RuleMatch) + Send + Sync + 'static,
	) -> Result<RuleId, RuleError> {
		let id = self.add(rule, handler)?;
		self.prologue.insert(id);
		Ok(id)
	}

	/// Adds a hook called for every resolved import, which loads what the module exports.
	pub fn on_import(
		&mut self,
		hook: impl Fn(&mut Lower<'_, '_>, &Import) + Send + Sync + 'static,
	) {
		self.import_hooks.push(Arc::new(hook));
	}

	pub(crate) fn import_hooks(&self) -> &[ImportHook] {
		&self.import_hooks
	}

	pub fn rules(&self) -> &RuleSet {
		&self.rules
	}
}

/// The result of compiling a source.
#[derive(Debug)]
pub struct Compiled {
	pub unit: Unit,
	/// The parse tree that was lowered: the prologue, then the body (parsed again with the
	/// extended grammar, if the prologue extended it). For editors.
	pub tree: Vec<Node>,
	/// The unit's grammar, if its prologue extended the base one: the rules of `tree` are
	/// in it.
	pub language: Option<Arc<Language>>,
	/// Parse errors and errors found while lowering. A unit with errors is not committed to
	/// the workspace.
	pub diagnostics: Vec<Diagnostic>,
}

/// Parses a source and lowers it into a unit. `path` is the source's path in the
/// workspace; `resolver` finds the files it includes and the modules it imports.
pub fn compile(
	language: &Language,
	source: Arc<Source>,
	path: &str,
	resolver: Arc<dyn Resolver>,
) -> Compiled {
	let parsed = nml_grammar::parse(&language.rules, source.clone());
	let mut lower = Lower {
		base: language,
		extended: None,
		resolver,
		builder: DocumentBuilder::new(source.clone()),
		exports: Vec::new(),
		uses: Vec::new(),
		dependencies: Vec::new(),
		assets: Vec::new(),
		diagnostics: Vec::new(),
		variables: HashMap::new(),
		prologue: true,
		files: vec![path.to_string()],
		imports: Vec::new(),
		exported: Vec::new(),
		extensions: HashMap::new(),
		finishers: Vec::new(),
	};
	let mut parse_diagnostics = parsed.diagnostics;
	let mut nodes = parsed.nodes;
	let mut tree = Vec::new();
	let mut index = 0;
	while index < nodes.len() {
		// The prologue ends here: if it extended the grammar, the body is parsed again with
		// the extended grammar.
		if lower.prologue && !lower.is_prologue_node(&nodes[index]) {
			lower.prologue = false;
			if let Some(extended) = lower.extended.clone() {
				let start = nodes[index].token().start();
				let reparsed = nml_grammar::parse_from(extended.rules(), source.clone(), start);
				parse_diagnostics.retain(|diagnostic| {
					let location = &diagnostic.primary().location;
					!Arc::ptr_eq(location.source(), &source) || location.start() < start
				});
				parse_diagnostics.extend(reparsed.diagnostics);
				nodes = reparsed.nodes;
				index = 0;
				continue;
			}
		}
		let len = lower.run_len(&nodes[index..]);
		lower.nodes(&nodes[index..index + len]);
		tree.extend_from_slice(&nodes[index..index + len]);
		index += len;
	}
	// Finishers may add more finishers.
	while !lower.finishers.is_empty() {
		for finisher in std::mem::take(&mut lower.finishers) {
			finisher(&mut lower);
		}
	}
	let Lower {
		builder,
		exports,
		uses,
		dependencies,
		assets,
		diagnostics: lowering_diagnostics,
		mut variables,
		extended,
		..
	} = lower;
	let mut diagnostics = parse_diagnostics;
	diagnostics.extend(lowering_diagnostics);
	Compiled {
		unit: Unit {
			reference_key: variables
				.remove(crate::variable::REFERENCE_KEY)
				.map(|variable| variable.value.content().to_string()),
			document: builder.finish(),
			exports,
			uses,
			dependencies,
			assets,
		},
		tree,
		language: extended,
		diagnostics,
	}
}

/// The state of lowering a parse tree into a unit, given to rule handlers.
pub struct Lower<'a, 'l> {
	base: &'a Language,
	/// The unit's grammar, if its prologue extended the base one.
	extended: Option<Arc<Language>>,
	pub resolver: Arc<dyn Resolver>,
	pub builder: DocumentBuilder<'l>,
	pub exports: Vec<Export>,
	pub uses: Vec<Use>,
	pub dependencies: Vec<Dependency>,
	/// Workspace files the page shows, copied to the output with it.
	pub assets: Vec<String>,
	pub diagnostics: Vec<Diagnostic>,
	/// Variables set with `:set` or `:export`, and those exported by imported modules
	/// (`namespace.name`).
	pub variables: HashMap<String, Variable>,
	/// Whether the lowering is still in the prologue.
	pub prologue: bool,
	/// The workspace paths of the unit's file and of the files being included in it, the
	/// innermost last.
	pub files: Vec<String>,
	/// Namespaces of the imported modules, and where they are written.
	pub imports: Vec<(String, Token)>,
	/// Exported names and where they are written.
	exported: Vec<(String, Token)>,
	/// State that other crates keep for the unit, by type (the Lua runtime).
	extensions: HashMap<TypeId, Box<dyn Any>>,
	/// Run when the lowering ends, in the order they were added.
	finishers: Vec<Finisher>,
}

/// A variable: where its name and its value are written.
#[derive(Debug, Clone)]
pub struct Variable {
	pub name: Token,
	/// The value as written: markup, parsed where the variable is substituted.
	pub value: Token,
}

/// Code run when a unit's lowering ends.
pub type Finisher = Box<dyn FnOnce(&mut Lower<'_, '_>)>;

impl fmt::Debug for Lower<'_, '_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Lower")
			.field("builder", &self.builder)
			.finish_non_exhaustive()
	}
}

/// Why a rule cannot be added to a unit's grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddRuleError {
	/// Grammar extensions come from the prologue or imported modules only.
	OutsidePrologue,
	Rule(RuleError),
}

impl Lower<'_, '_> {
	/// The unit's grammar: the base one, extended by the prologue.
	pub fn language(&self) -> &Language {
		self.extended.as_deref().unwrap_or(self.base)
	}

	/// Adds a rule to the unit's grammar. Only allowed in the prologue: the body is parsed
	/// with the extended grammar.
	pub fn add_rule(
		&mut self,
		rule: Rule,
		handler: impl Fn(&mut Lower<'_, '_>, &RuleMatch) + Send + Sync + 'static,
	) -> Result<RuleId, AddRuleError> {
		if !self.prologue {
			return Err(AddRuleError::OutsidePrologue);
		}
		let base = self.base;
		let language = self.extended.get_or_insert_with(|| Arc::new(base.clone()));
		Arc::make_mut(language)
			.add(rule, handler)
			.map_err(AddRuleError::Rule)
	}

	fn is_prologue_node(&self, node: &Node) -> bool {
		matches!(node, Node::Match(m) if self.language().prologue.contains(&m.rule))
	}

	/// Lowers nodes into the open element. Adjacent text and escapes become one text element,
	/// and consecutive matches of grouped rules are lowered together.
	pub fn nodes(&mut self, nodes: &[Node]) {
		let mut text: Option<(Token, String)> = None;
		let mut index = 0;
		while index < nodes.len() {
			let node = &nodes[index];
			index += 1;
			if let Node::Match(m) = node
				&& m.complete
				&& let Some((_, handler)) = self.language().groups.get(&m.rule).cloned()
			{
				if self.builder.depth() == 1 && !self.is_prologue_node(node) {
					self.prologue = false;
				}
				self.flush(text.take());
				let len = self.run_len(&nodes[index - 1..]);
				let run: Vec<&RuleMatch> = nodes[index - 1..index - 1 + len]
					.iter()
					.map(|node| match node {
						Node::Match(m) => m,
						_ => unreachable!("runs are made of matches"),
					})
					.collect();
				index += len - 1;
				handler(self, &run);
				continue;
			}
			// A block at the top of the unit ends the prologue, unless its rule is allowed in it.
			if self.builder.depth() == 1 && !self.is_prologue_node(node) {
				self.prologue = false;
			}
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
				Node::Comment(_) => {}
				Node::Paragraph { token, children } => {
					self.flush(text.take());
					self.builder.open(ElementKind::Paragraph, token.clone());
					self.nodes(children);
					self.builder.close();
				}
				Node::Match(m) => {
					self.flush(text.take());
					if m.complete {
						let handler = self.language().handlers[&m.rule].clone();
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

	/// How many of the nodes, from the first, are lowered together: the run of matches of
	/// grouped rules with the first one's group key, or just the first node.
	fn run_len(&self, nodes: &[Node]) -> usize {
		let groups = &self.language().groups;
		let key = |node: &Node| match node {
			Node::Match(m) if m.complete => groups.get(&m.rule).map(|(key, _)| key(m)),
			_ => None,
		};
		let Some(group) = nodes.first().and_then(key) else {
			return 1.min(nodes.len());
		};
		1 + nodes[1..]
			.iter()
			.take_while(|node| key(node).as_ref() == Some(&group))
			.count()
	}

	/// Lowers the nodes of a capture, if it holds any.
	pub fn capture(&mut self, m: &RuleMatch, name: &str) {
		if let Some(Value::Nodes(nodes)) = m.capture(name).map(|capture| &capture.value) {
			self.nodes(nodes);
		}
	}

	/// The unit's state of type `T`, created by `init` the first time it is asked for.
	pub fn extension<T: 'static>(&mut self, init: impl FnOnce() -> T) -> &mut T {
		self.extensions
			.entry(TypeId::of::<T>())
			.or_insert_with(|| Box::new(init()))
			.downcast_mut()
			.expect("extensions are keyed by their type")
	}

	/// The unit's state of type `T`, if it was created.
	pub fn extension_mut<T: 'static>(&mut self) -> Option<&mut T> {
		self.extensions.get_mut(&TypeId::of::<T>())?.downcast_mut()
	}

	/// Runs `finisher` when the lowering ends, after every node is lowered.
	pub fn on_finish(&mut self, finisher: impl FnOnce(&mut Lower<'_, '_>) + 'static) {
		self.finishers.push(Box::new(finisher));
	}

	/// Records a parse-time dependency, once.
	pub fn depend(&mut self, dependency: Dependency) {
		if !self.dependencies.contains(&dependency) {
			self.dependencies.push(dependency);
		}
	}

	/// Adds an export, unless the unit already exports that name. `reference` is where the
	/// name is written: it becomes the export's declaration.
	pub fn export(&mut self, mut export: Export, reference: Token) {
		let file = self.files.last().unwrap().clone();
		export.declared = Some((file, reference.root().range()));
		if let Some((_, first)) = self.exported.iter().find(|(name, _)| *name == export.name) {
			let kind = DiagnosticKind::DuplicateReference {
				name: export.name.clone(),
			};
			let diagnostic = Diagnostic::new(kind, DiagnosticMessage::new(reference))
				.with(DiagnosticMessage::fluent(first.clone(), "first"));
			self.diagnostics.push(diagnostic);
			return;
		}
		self.exported.push((export.name.clone(), reference));
		self.exports.push(export);
	}

	fn flush(&mut self, text: Option<(Token, String)>) {
		if let Some((token, text)) = text {
			self.builder.leaf(ElementKind::Text(text), token);
		}
	}
}

/// The text of nodes without markup, with escapes resolved.
pub(crate) fn plain_text(nodes: &[Node]) -> String {
	let mut text = String::new();
	collect_text(nodes, &mut text);
	text
}

fn collect_text(nodes: &[Node], out: &mut String) {
	for node in nodes {
		match node {
			Node::Text(token) => out.push_str(token.content()),
			Node::Escape(token) => out.push_str(&token.content()[1..]),
			Node::Comment(_) => {}
			Node::Paragraph { children, .. } => collect_text(children, out),
			Node::Match(m) => {
				for capture in &m.captures {
					if let Value::Nodes(nodes) = &capture.value {
						collect_text(nodes, out);
					}
				}
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use nml_document::ElementKind;
	use nml_grammar::Atom;
	use nml_grammar::Rule;
	use nml_source::Source;
	use nml_source::Url;

	use crate::Language;
	use crate::MemoryResolver;
	use crate::compile;
	use crate::tests::show;

	#[test]
	fn extensions_live_for_the_unit() {
		let mut language = Language::empty();
		language
			.add(Rule::new("count", vec![Atom::literal("@")]), |lower, m| {
				let count = lower.extension(|| 0usize);
				*count += 1;
				let text = count.to_string();
				lower.builder.leaf(ElementKind::Text(text), m.token.clone());
			})
			.unwrap();
		let source = Source::new(Url::parse("file:///a.nml").unwrap(), "@ @ @".into(), 0);
		let compiled = compile(
			&language,
			Arc::new(source),
			"a.nml",
			Arc::new(MemoryResolver::default()),
		);
		let document = &compiled.unit.document;
		assert_eq!(
			show(document, document.root()),
			r#"document[paragraph["1" " " "2" " " "3"]]"#
		);
	}

	#[test]
	fn finishers_run_at_the_end() {
		let mut language = Language::empty();
		language
			.add(Rule::new("mark", vec![Atom::literal("@")]), |lower, m| {
				let token = m.token.clone();
				lower.on_finish(move |lower| {
					// Runs after the paragraph is closed: the text goes at the end.
					lower
						.builder
						.leaf(ElementKind::Text("!".into()), token.clone());
					lower.on_finish(move |lower| {
						lower.builder.leaf(ElementKind::Text("?".into()), token);
					});
				});
			})
			.unwrap();
		let source = Source::new(Url::parse("file:///a.nml").unwrap(), "a @ b".into(), 0);
		let compiled = compile(
			&language,
			Arc::new(source),
			"a.nml",
			Arc::new(MemoryResolver::default()),
		);
		let document = &compiled.unit.document;
		assert_eq!(
			show(document, document.root()),
			r#"document[paragraph["a " " b"] "!" "?"]"#
		);
	}

	/// A language whose `:enable` command, in the prologue, adds a rule for `@`.
	fn extensible() -> Language {
		let mut language = Language::empty();
		language
			.add_prologue(
				Rule::new(
					"enable",
					vec![Atom::LineStart, Atom::literal(":enable"), Atom::LineEnd],
				),
				|lower, m| {
					let result =
						lower.add_rule(Rule::new("at", vec![Atom::literal("@")]), |lower, m| {
							lower
								.builder
								.leaf(ElementKind::Text("AT".into()), m.token.clone());
						});
					if let Err(error) = result {
						let text = format!("{error:?}");
						lower.builder.leaf(ElementKind::Text(text), m.token.clone());
					}
				},
			)
			.unwrap();
		language
	}

	fn compile_text(language: &Language, text: &str) -> crate::Compiled {
		let source = Source::new(Url::parse("file:///a.nml").unwrap(), text.into(), 0);
		compile(
			language,
			Arc::new(source),
			"a.nml",
			Arc::new(MemoryResolver::default()),
		)
	}

	#[test]
	fn the_prologue_extends_the_grammar() {
		let language = extensible();
		let compiled = compile_text(&language, ":enable\n\na @ b\n\n:enable\n");
		let document = &compiled.unit.document;
		assert_eq!(
			show(document, document.root()),
			r#"document[paragraph["a " "AT" " b"] "OutsidePrologue"]"#
		);
		assert!(compiled.diagnostics.is_empty());

		// The base language is not changed.
		assert!(language.rules().id("at").is_none());
		let compiled = compile_text(&language, "a @ b");
		let document = &compiled.unit.document;
		assert_eq!(
			show(document, document.root()),
			r#"document[paragraph["a @ b"]]"#
		);
	}

	#[test]
	fn errors_of_the_first_parse_are_dropped_with_it() {
		// With the base grammar, `<` opens a construct that `>` must close. The prologue
		// replaces that rule with a plain `<`, so the body's unclosed `<` is no error.
		let mut language = Language::empty();
		language
			.add(
				Rule::new(
					"angle",
					vec![Atom::literal("<"), Atom::Text, Atom::literal(">")],
				),
				|_, _| {},
			)
			.unwrap();
		language
			.add_prologue(
				Rule::new(
					"plain",
					vec![Atom::LineStart, Atom::literal(":plain"), Atom::LineEnd],
				),
				|lower, _| {
					let rule = Rule::new("lt", vec![Atom::literal("<")]).replacing("angle");
					lower
						.add_rule(rule, |lower, m| {
							lower
								.builder
								.leaf(ElementKind::Text("<".into()), m.token.clone());
						})
						.unwrap();
				},
			)
			.unwrap();

		let compiled = compile_text(&language, ":plain\n\na < b\n");
		assert!(
			compiled.diagnostics.is_empty(),
			"{:?}",
			compiled.diagnostics
		);
		let compiled = compile_text(&language, "a < b\n");
		nml_diagnostics::assert_diagnostics!(compiled.diagnostics, [1001]);
	}
}
