use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

use parking_lot::RwLock;
use tower_lsp::lsp_types::Diagnostic;

use crate::parser::source::Source;
use crate::parser::source::SourceFile;
use crate::parser::source::SourcePosition;
use crate::parser::source::Token;

use super::code::CodeRangeData;
use super::conceal::ConcealData;
use super::definition;
use super::definition::DefinitionData;
use super::hints::HintsData;
use super::hover::Hover;
use super::hover::HoverData;
use super::hover::HoverRange;
use super::ranges::CustomRange;
use super::ranges::Range;
use super::ranges::RangeData;
use super::reference::LsReference;
use super::semantic::Semantics;
use super::semantic::SemanticsData;
use super::semantic::Tokens;
use super::styles::StylesData;

/// Stores data for a translation unit that will be passed to the language server
#[derive(Default)]
pub struct LangServerData {
	/// Available semantic tokens
	pub semantic_tokens: Tokens,
	/// List of semantic tokens for this translatiop unit
	pub semantic_data: HashMap<Arc<dyn Source>, SemanticsData>,
	pub diagnostics: HashMap<Arc<dyn Source>, Vec<Diagnostic>>,
	pub inlay_hints: HashMap<Arc<dyn Source>, HintsData>,
	pub definitions: HashMap<Arc<dyn Source>, DefinitionData>,
	pub hovers: HashMap<Arc<dyn Source>, HoverData>,
	pub conceals: HashMap<Arc<dyn Source>, ConcealData>,
	pub styles: HashMap<Arc<dyn Source>, StylesData>,
	pub coderanges: HashMap<Arc<dyn Source>, CodeRangeData>,
	pub ranges: HashMap<Arc<dyn Source>, RangeData>,

	pub external_refs: HashMap<String, LsReference>,

	pub sources: RwLock<HashMap<String, Arc<dyn Source>>>,
}

impl LangServerData {
	/// Method that must be called when a source is added
	pub fn on_new_source(&mut self, source: Arc<dyn Source>) {
		if source.downcast_ref::<SourceFile>().is_some() {
			self.sources
				.write()
				.insert(source.name().to_owned(), source.clone());
		}

		if !self.semantic_data.contains_key(&source) {
			self.semantic_data
				.insert(source.clone(), SemanticsData::new(source.clone()));
		}
		if !self.diagnostics.contains_key(&source) {
			self.diagnostics.insert(source.clone(), Vec::default());
		}
		if !self.inlay_hints.contains_key(&source) {
			self.inlay_hints
				.insert(source.clone(), HintsData::new(source.clone()));
		}
		if !self.definitions.contains_key(&source) {
			self.definitions
				.insert(source.clone(), DefinitionData::default());
		}
		if !self.hovers.contains_key(&source) {
			self.hovers.insert(source.clone(), HoverData::default());
		}
		if !self.conceals.contains_key(&source) {
			self.conceals.insert(source.clone(), ConcealData::default());
		}
		if !self.styles.contains_key(&source) {
			self.styles.insert(source.clone(), StylesData::default());
		}
		if !self.coderanges.contains_key(&source) {
			self.coderanges
				.insert(source.clone(), CodeRangeData::default());
		}
	}

	pub fn on_source_end(&mut self, source: Arc<dyn Source>) {
		if source.content().is_empty() {
			return;
		}
		// Process the rest of the semantic queue for the current source
		let pos = source.original_position(source.content().len() - 1).1;
		if let Some(sems) = Semantics::from_source(source, self) {
			sems.process_queue(pos);
		}
	}

	/// Gets a source file by name, or insert a new file
	pub fn get_source<'lsp>(&'lsp self, name: &str) -> Option<Arc<dyn Source>> {
		if let Some(found) = self.sources.read().get(name) {
			return Some(found.to_owned());
		}

		let Ok(file) = SourceFile::new(name.to_string(), None) else {
			return None;
		};
		let source = Arc::new(file);
		self.sources
			.write()
			.insert(source.name().to_owned(), source.clone());
		Some(source)
	}

	pub fn with_semantics<'lsp, F, R>(&'lsp self, source: Arc<dyn Source>, f: F) -> Option<R>
	where
		F: FnOnce(&Semantics, &'lsp Tokens) -> R,
	{
		match Semantics::from_source(source, self) {
			Some(sems) => Some(f(&sems, &self.semantic_tokens)),
			None => None,
		}
	}

	pub fn add_definition<'lsp>(&'lsp self, source: Token, target: &Token) {
		definition::from_source(source, target, self);
	}

	pub fn add_hover<'lsp>(&'lsp self, range: Token, content: String) {
		let Some(hov) = Hover::from_source(range.source(), self) else {
			return;
		};
		let original = range.source().original_range(range.range);
		hov.add(HoverRange {
			range: original,
			content,
		});
	}
	
	pub fn add_range<'lsp>(&'lsp self, source: Arc<dyn Source>, range: std::ops::Range<usize>, data: CustomRange)
	{
		let Some(r) = Range::from_source(source.clone(), self) else { return };
		let original = source.original_range(range);
		r.add(original.range, data);
	}
}
