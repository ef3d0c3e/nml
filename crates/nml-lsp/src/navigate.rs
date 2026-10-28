//! Hover, go-to-definition and references.

use std::str::FromStr;
use std::sync::Arc;

use lsp_types::Hover;
use lsp_types::HoverContents;
use lsp_types::Location;
use lsp_types::MarkupContent;
use lsp_types::MarkupKind;
use lsp_types::TextDocumentPositionParams;
use lsp_types::Uri;
use nml_build::LinkError;
use nml_build::Target;
use nml_diagnostics::DiagnosticKind;
use nml_document::ElementKind;
use nml_document::Scope;
use nml_document::SymbolRef;
use nml_lang::Resolver;
use nml_source::Encoding;
use nml_source::Source;
use nml_source::Token;
use nml_source::Url;
use nml_workspace::FileState;
use nml_workspace::Index;
use nml_workspace::Role;
use nml_workspace::UnitRecord;

use crate::Analysis;
use crate::Result;
use crate::Server;
use crate::convert;
use crate::find;

/// What is under the cursor, for navigation.
enum Subject {
	/// A reference `[[…]]`, and the token of its target.
	Reference(SymbolRef, Token),
	/// The name of an exported symbol, where it is declared.
	Declaration(String, Token),
	/// A file named by `:include` or `:import`: its workspace path.
	File(String),
}

impl Server {
	pub(crate) fn analysis(
		&self,
		position: &TextDocumentPositionParams,
	) -> Result<Option<(&Analysis, usize, Url)>> {
		let url = Url::parse(position.text_document.uri.as_str())?;
		let Some(analysis) = self.analyses.get(&url) else {
			return Ok(None);
		};
		let position = nml_source::Position::new(
			position.position.line as usize,
			position.position.character as usize,
		);
		let offset = analysis.source.offset(position, Encoding::Utf16);
		Ok(offset.map(|offset| (analysis, offset, url)))
	}

	/// What is under the cursor: the innermost match that navigation knows.
	fn subject(&self, position: &TextDocumentPositionParams) -> Result<Option<Subject>> {
		let Some((analysis, offset, url)) = self.analysis(position)? else {
			return Ok(None);
		};
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		for hit in find::at(&analysis.tree, &url, offset).iter().rev() {
			let Some(rule) = language.rules().get(hit.matched.rule) else {
				continue;
			};
			let capture = |name: &str| hit.matched.capture(name).map(|c| c.token.clone());
			let subject = match rule.name.as_str() {
				"reference" => capture("target").and_then(|token| {
					Some(Subject::Reference(
						SymbolRef::parse(token.content())?,
						token,
					))
				}),
				"heading" => match hit.capture {
					Some(c) if &*c.name == "ref" => Some(Subject::Declaration(
						c.token.content().to_string(),
						c.token.clone(),
					)),
					_ => None,
				},
				"include" => capture("path").and_then(|token| {
					nml_lang::resolve_path(&analysis.path, token.content().trim_end())
						.map(Subject::File)
				}),
				"import" => capture("module").and_then(|token| {
					let resolver = self.resolver();
					resolver
						.module(token.content())
						.ok()
						.map(|(path, _)| Subject::File(path))
				}),
				_ => None,
			};
			if subject.is_some() {
				return Ok(subject);
			}
		}
		Ok(None)
	}

	/// The workspace index, if a build created it.
	pub(crate) fn index(&self) -> Option<Index> {
		let path = self.root.as_ref()?.join(&self.config.as_ref()?.database);
		path.exists().then(|| Index::open(&path).ok()).flatten()
	}

	/// Where a workspace path is on disk. A module that comes with NML, `std/<name>.nml`
	/// (unless the workspace has its own `std`), is its copy in `.nml/std`.
	pub(crate) fn disk_path(&self, path: &str) -> Option<std::path::PathBuf> {
		let root = self.root.as_ref()?;
		let absolute = root.join(path);
		let own_std = (self.config.as_ref())
			.is_some_and(|config| config.modules.iter().any(|module| module.name == "std"));
		match path.strip_prefix("std/") {
			Some(rest) if !own_std && !absolute.exists() => {
				Some(root.join(nml_lua::STD_DIR).join(rest))
			}
			_ => Some(absolute),
		}
	}

	/// The source of a workspace file: its open buffer, or the disk.
	pub(crate) fn file(&self, path: &str) -> Option<Arc<Source>> {
		let absolute = self.disk_path(path)?;
		let url = Url::from_file_path(&absolute).ok()?;
		let content = match self.buffers.get(&url) {
			Some(content) => content.clone(),
			None => std::fs::read_to_string(&absolute).ok()?,
		};
		Some(Arc::new(Source::new(url, content, 0)))
	}

	pub(crate) fn location(&self, path: &str, range: std::ops::Range<usize>) -> Option<Location> {
		let source = self.file(path)?;
		let token = Token::new(
			source.clone(),
			range.start.min(source.content().len())..range.end.min(source.content().len()),
		);
		Some(Location::new(
			Uri::from_str(source.location().as_str()).ok()?,
			convert::range(&token),
		))
	}

	/// The document's unit: its record in the index, or one made from its analysis if it is
	/// not built yet.
	pub(crate) fn current_unit(&self, analysis: &Analysis) -> UnitRecord {
		self.index()
			.and_then(|index| index.unit(&analysis.path).ok().flatten())
			.unwrap_or_else(|| UnitRecord {
				id: -1,
				path: analysis.path.clone(),
				name: analysis.key.clone(),
				title: analysis.key.clone(),
				role: Role::Page,
				state: FileState {
					mtime: 0,
					size: 0,
					hash: Vec::new(),
				},
				output: nml_lang::output_path(&analysis.path),
				output_range: None,
			})
	}

	/// Resolves a reference written in the document, as the link stage would: in the
	/// document itself for `[[name]]`, in the index otherwise.
	fn resolve(
		&self,
		analysis: &Analysis,
		target: &SymbolRef,
	) -> std::result::Result<Target, LinkError> {
		let index = self.index();
		let current = self.current_unit(analysis);
		if target.scope == Scope::Local {
			// The buffer may not be saved: its own symbols come from its analysis.
			let export = analysis
				.exports
				.iter()
				.find(|export| export.name == target.name);
			return match export {
				Some(export) => Ok(Target {
					unit: current,
					export: Some(export.clone()),
				}),
				None => Err(LinkError::Unresolved),
			};
		}
		match index {
			Some(index) => nml_build::resolve(&index, &current, target),
			None => Err(LinkError::Unresolved),
		}
	}

	pub(crate) fn hover(&self, position: &TextDocumentPositionParams) -> Result<Option<Hover>> {
		// A footnote's reference: its note.
		if let Some((value, token)) = self.footnote_hover(position)? {
			return Ok(Some(Hover {
				contents: HoverContents::Markup(MarkupContent {
					kind: MarkupKind::Markdown,
					value,
				}),
				range: Some(convert::range(&token)),
			}));
		}
		// A `:run` block's first line: its result.
		if let Some((_, offset, url)) = self.analysis(position)?
			&& let Some((value, token)) = self.run_hover(&url, offset)
		{
			return Ok(Some(Hover {
				contents: HoverContents::Markup(MarkupContent {
					kind: MarkupKind::Markdown,
					value,
				}),
				range: Some(convert::range(&token)),
			}));
		}
		let Some(Subject::Reference(target, token)) = self.subject(position)? else {
			return self.syntax_hover(position);
		};
		let analysis = self.analysis(position)?.unwrap().0;
		let value = match self.resolve(analysis, &target) {
			Ok(Target {
				unit,
				export: Some(export),
			}) => {
				let title = export.property("title").unwrap_or(&export.name);
				let mut value = format!("**{title}**\n\n`{}` in `{}`", export.name, unit.path);
				// A figure's medium, which editors can show.
				if let Some(media) = export.property("media")
					&& let Some(root) = &self.root
					&& let Ok(url) = Url::from_file_path(root.join(media))
				{
					value.push_str(&format!("\n\n![{title}]({url})"));
				}
				value
			}
			Ok(Target { unit, export: None }) => format!("**{}**\n\n`{}`", unit.title, unit.path),
			Err(error) => {
				let target = target.to_string();
				let kind = match error {
					LinkError::Ambiguous(count) => {
						DiagnosticKind::AmbiguousReference { target, count }
					}
					_ => DiagnosticKind::UnresolvedReference { target },
				};
				self.localizer.title(&kind)
			}
		};
		Ok(Some(Hover {
			contents: HoverContents::Markup(MarkupContent {
				kind: MarkupKind::Markdown,
				value,
			}),
			range: Some(convert::range(&token)),
		}))
	}

	/// The documentation of the syntax under the cursor: a variable's value, or the rule of
	/// the innermost match, when the cursor is on its markup (not on its content).
	fn syntax_hover(&self, position: &TextDocumentPositionParams) -> Result<Option<Hover>> {
		let Some((analysis, offset, url)) = self.analysis(position)? else {
			return Ok(None);
		};
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		// A match that failed may run on past what is being written (`%name` unclosed): it
		// documents nothing.
		let hits = find::at(&analysis.tree, &url, offset);
		let Some(hit) = hits.iter().rev().find(|hit| hit.matched.complete) else {
			return Ok(None);
		};
		let Some(rule) = language.rules().get(hit.matched.rule) else {
			return Ok(None);
		};
		let capture = hit.capture.map(|capture| &*capture.name);
		// The module of an import: its documentation.
		if let ("import", Some("module")) = (rule.name.as_str(), capture) {
			let token = hit.capture.unwrap().token.clone();
			let Some(value) = self.module_hover(analysis, token.content()) else {
				return Ok(None);
			};
			return Ok(Some(Hover {
				contents: HoverContents::Markup(MarkupContent {
					kind: MarkupKind::Markdown,
					value,
				}),
				range: Some(convert::range(&token)),
			}));
		}
		let variable = match (rule.name.as_str(), capture) {
			("substitution", _) | ("set" | "export", Some("name")) => {
				hit.matched.capture("name").map(|c| c.token.clone())
			}
			_ => None,
		};
		let (value, token) = match variable {
			Some(name) => {
				let value = match analysis.variables.get(name.content()) {
					Some(variable) => self.variable_doc(name.content(), variable),
					None => self.localizer.title(&DiagnosticKind::UndefinedVariable {
						name: name.content().into(),
					}),
				};
				(value, name)
			}
			None => {
				// In content, or in code: what is there is not the rule's markup.
				if let Some(capture) = hit.capture
					&& (matches!(capture.value, nml_grammar::Value::Nodes(_))
						|| matches!(&*capture.name, "code" | "content" | "source" | "value"))
				{
					return Ok(None);
				}
				if rule.doc.is_empty() {
					return Ok(None);
				}
				let token = hit.capture.map_or(&hit.matched.token, |c| &c.token);
				(rule.doc.clone(), token.clone())
			}
		};
		Ok(Some(Hover {
			contents: HoverContents::Markup(MarkupContent {
				kind: MarkupKind::Markdown,
				value,
			}),
			range: Some(convert::range(&token)),
		}))
	}

	pub(crate) fn definition(
		&self,
		position: &TextDocumentPositionParams,
	) -> Result<Option<Location>> {
		if let Some(location) = self.footnote_definition(position)? {
			return Ok(Some(location));
		}
		let Some(subject) = self.subject(position)? else {
			return self.syntax_definition(position);
		};
		let analysis = self.analysis(position)?.unwrap().0;
		Ok(match subject {
			Subject::Reference(target, _) => match self.resolve(analysis, &target) {
				Ok(Target {
					export: Some(export),
					..
				}) => export
					.declared
					.and_then(|(path, range)| self.location(&path, range)),
				Ok(Target { unit, export: None }) => self.location(&unit.path, 0..0),
				Err(_) => None,
			},
			Subject::Declaration(_, token) => Some(Location::new(
				position.text_document.uri.clone(),
				convert::range(&token),
			)),
			Subject::File(path) => self.location(&path, 0..0),
		})
	}

	/// The documentation of a module: its name, its own documentation, and the syntax it
	/// adds to the document's grammar.
	fn module_hover(&self, analysis: &Analysis, module: &str) -> Option<String> {
		let (path, source) = self.resolver().module(module).ok()?;
		let mut value = format!("**{module}**\n\n`{path}`");
		let doc = nml_lang::module_doc(source.content());
		if !doc.is_empty() {
			value.push_str("\n\n");
			value.push_str(&doc);
		}
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let mut syntax = Vec::new();
		for (_, rule) in language.rules().iter() {
			let Some(origin) = &rule.origin else {
				continue;
			};
			if origin.root().source().location() != source.location() {
				continue;
			}
			// The first paragraph of its documentation.
			let summary = rule.doc.split("\n\n").next().unwrap_or_default();
			syntax.push(match summary.is_empty() {
				true => format!("* `{}`", rule.name),
				false => format!("* `{}`: {summary}", rule.name),
			});
		}
		if !syntax.is_empty() {
			value.push_str("\n\n---\n\n**Syntax**\n\n");
			value.push_str(&syntax.join("\n"));
		}
		Some(value)
	}

	/// Where the syntax under the cursor is defined: the `nml.rule` call of a rule from a
	/// module or a prologue, when the cursor is on its markup. Built-in syntax has none.
	fn syntax_definition(&self, position: &TextDocumentPositionParams) -> Result<Option<Location>> {
		let Some((analysis, offset, url)) = self.analysis(position)? else {
			return Ok(None);
		};
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let hits = find::at(&analysis.tree, &url, offset);
		let Some(hit) = hits.iter().rev().find(|hit| hit.matched.complete) else {
			return Ok(None);
		};
		// In content: what is there is not the rule's markup.
		if hit
			.capture
			.is_some_and(|capture| matches!(capture.value, nml_grammar::Value::Nodes(_)))
		{
			return Ok(None);
		}
		let origin = language
			.rules()
			.get(hit.matched.rule)
			.and_then(|rule| rule.origin.as_ref());
		Ok(origin.and_then(|origin| self.token_location(origin)))
	}

	/// The location of a token, in the file at the end of its source's chain. The modules
	/// that come with NML (`nml:///std/…`) are their copies on disk.
	pub(crate) fn token_location(&self, token: &Token) -> Option<Location> {
		let token = token.root();
		let location = token.source().location();
		let url = match location.scheme() {
			"nml" => {
				let path = location.path().trim_start_matches('/');
				Url::from_file_path(self.disk_path(path)?).ok()?
			}
			_ => location.clone(),
		};
		Some(Location::new(
			Uri::from_str(url.as_str()).ok()?,
			convert::range(&token),
		))
	}

	pub(crate) fn references(
		&self,
		position: &TextDocumentPositionParams,
		include_declaration: bool,
	) -> Result<Vec<Location>> {
		let Some(subject) = self.subject(position)? else {
			return Ok(Vec::new());
		};
		let analysis = self.analysis(position)?.unwrap().0;
		// The symbol: its unit's key, its name, and where it is declared.
		let (key, name, declared) = match subject {
			Subject::Declaration(name, token) => {
				let declared = (analysis.path.clone(), token.root().range());
				(analysis.key.clone(), name, Some(declared))
			}
			Subject::Reference(target, _) => match self.resolve(analysis, &target) {
				Ok(Target {
					unit,
					export: Some(export),
				}) => (unit.name, export.name, export.declared),
				_ => return Ok(Vec::new()),
			},
			Subject::File(_) => return Ok(Vec::new()),
		};
		let mut locations = Vec::new();
		if include_declaration && let Some((path, range)) = declared {
			locations.extend(self.location(&path, range));
		}
		if let Some(index) = self.index() {
			for (path, range) in index.uses_of(&key, &name)? {
				locations.extend(self.location(&path, range));
			}
		}
		Ok(locations)
	}
}

impl Server {
	/// Hints after each reference without a caption: the label it resolves to.
	pub(crate) fn inlay_hints(
		&self,
		params: &lsp_types::InlayHintParams,
	) -> Result<Vec<lsp_types::InlayHint>> {
		let url = Url::parse(params.text_document.uri.as_str())?;
		let Some(analysis) = self.analyses.get(&url) else {
			return Ok(Vec::new());
		};
		let language = analysis.language.as_deref().unwrap_or(&self.language);
		let mut matches = Vec::new();
		references(&analysis.tree, language, &mut matches);
		let mut hints = Vec::new();
		for m in matches {
			if m.capture("caption").is_some() {
				continue;
			}
			let root = m.token.root();
			if root.source().location() != &url {
				continue;
			}
			let range = convert::range(&root);
			if range.end < params.range.start || params.range.end < range.start {
				continue;
			}
			let Some(target) = m
				.capture("target")
				.and_then(|c| SymbolRef::parse(c.token.content()))
			else {
				continue;
			};
			let label = match self.resolve(analysis, &target) {
				Ok(target) => nml_build::label(&target),
				Err(_) => continue,
			};
			hints.push(lsp_types::InlayHint {
				position: range.end,
				label: lsp_types::InlayHintLabel::String(format!(": {label}")),
				kind: None,
				text_edits: None,
				tooltip: None,
				padding_left: Some(true),
				padding_right: None,
				data: None,
			});
		}
		// Progress cookies: their count.
		let document = &analysis.document;
		for (id, element) in document.descendants(document.root()) {
			let ElementKind::Progress { percent } = element.kind else {
				continue;
			};
			let root = element.token.root();
			if root.source().location() != &url {
				continue;
			}
			let range = convert::range(&root);
			if range.end < params.range.start || params.range.end < range.start {
				continue;
			}
			let (done, total) = document.progress(id).unwrap_or((0, 0));
			let label = nml_document::progress_label(done, total, percent);
			hints.push(lsp_types::InlayHint {
				position: range.end,
				label: lsp_types::InlayHintLabel::String(label),
				kind: None,
				text_edits: None,
				tooltip: None,
				padding_left: Some(true),
				padding_right: None,
				data: None,
			});
		}
		Ok(hints)
	}
}

/// The matches of the `reference` rule in a tree, in order.
fn references<'t>(
	nodes: &'t [nml_grammar::Node],
	language: &nml_lang::Language,
	out: &mut Vec<&'t nml_grammar::RuleMatch>,
) {
	for node in nodes {
		match node {
			nml_grammar::Node::Paragraph { children, .. } => references(children, language, out),
			nml_grammar::Node::Match(m) => {
				if language
					.rules()
					.get(m.rule)
					.is_some_and(|rule| rule.name == "reference")
				{
					out.push(m);
				}
				for capture in &m.captures {
					if let nml_grammar::Value::Nodes(nodes) = &capture.value {
						references(nodes, language, out);
					}
				}
			}
			_ => {}
		}
	}
}
