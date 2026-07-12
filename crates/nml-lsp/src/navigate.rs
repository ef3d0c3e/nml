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
	fn analysis(
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

	fn resolver(&self) -> nml_build::WorkspaceResolver {
		let root = self.root.clone().unwrap_or_else(|| "/".into());
		let modules = self
			.config
			.as_ref()
			.map(|config| config.modules.clone())
			.unwrap_or_default();
		nml_build::WorkspaceResolver::new(root, modules)
	}

	/// The workspace index, if a build created it.
	pub(crate) fn index(&self) -> Option<Index> {
		let path = self.root.as_ref()?.join(&self.config.as_ref()?.database);
		path.exists().then(|| Index::open(&path).ok()).flatten()
	}

	/// The source of a workspace file: its open buffer, or the disk.
	pub(crate) fn file(&self, path: &str) -> Option<Arc<Source>> {
		let absolute = self.root.as_ref()?.join(path);
		let url = Url::from_file_path(&absolute).ok()?;
		let content = match self.buffers.get(&url) {
			Some(content) => content.clone(),
			None => std::fs::read_to_string(&absolute).ok()?,
		};
		Some(Arc::new(Source::new(url, content, 0)))
	}

	fn location(&self, path: &str, range: std::ops::Range<usize>) -> Option<Location> {
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

	/// Resolves a reference written in the document, as the link stage would: in the
	/// document itself for `[[name]]`, in the index otherwise.
	fn resolve(
		&self,
		analysis: &Analysis,
		target: &SymbolRef,
	) -> std::result::Result<Target, LinkError> {
		let index = self.index();
		let current = index
			.as_ref()
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
			});
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
		let Some(Subject::Reference(target, token)) = self.subject(position)? else {
			return Ok(None);
		};
		let analysis = self.analysis(position)?.unwrap().0;
		let value = match self.resolve(analysis, &target) {
			Ok(Target {
				unit,
				export: Some(export),
			}) => {
				let title = export.property("title").unwrap_or(&export.name);
				format!("**{title}**\n\n`{}` in `{}`", export.name, unit.path)
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

	pub(crate) fn definition(
		&self,
		position: &TextDocumentPositionParams,
	) -> Result<Option<Location>> {
		let Some(subject) = self.subject(position)? else {
			return Ok(None);
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
	/// Hints after each reference without a caption: the title it resolves to.
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
				Ok(Target {
					export: Some(export),
					..
				}) => export.property("title").unwrap_or(&export.name).to_string(),
				Ok(Target { unit, export: None }) => unit.title,
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
