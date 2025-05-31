use std::collections::HashMap;
use std::sync::Arc;

use crate::parser::source::Source;
use crate::parser::source::SourcePosition;
use crate::parser::source::Token;

use super::code::CodeRangeData;
use super::conceal::ConcealData;
use super::definition::DefinitionData;
use super::hints::HintsData;
use super::hover::Hover;
use super::hover::HoverData;
use super::hover::HoverRange;
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
	pub inlay_hints: HashMap<Arc<dyn Source>, HintsData>,
	pub definitions: HashMap<Arc<dyn Source>, DefinitionData>,
	pub hovers: HashMap<Arc<dyn Source>, HoverData>,
	pub conceals: HashMap<Arc<dyn Source>, ConcealData>,
	pub styles: HashMap<Arc<dyn Source>, StylesData>,
	pub coderanges: HashMap<Arc<dyn Source>, CodeRangeData>,
}

impl LangServerData {
	/// Method that must be called when a source is added
	pub fn on_new_source(&mut self, source: Arc<dyn Source>) {
		if !self.semantic_data.contains_key(&source) {
			self.semantic_data
				.insert(source.clone(), SemanticsData::new(source.clone()));
		}
		if !self.inlay_hints.contains_key(&source) {
			self.inlay_hints
				.insert(source.clone(), HintsData::new(source.clone()));
		}
		if !self.definitions.contains_key(&source) {
			self.definitions
				.insert(source.clone(), DefinitionData::new());
		}
		if !self.hovers.contains_key(&source) {
			self.hovers.insert(source.clone(), HoverData::default());
		}
		if !self.conceals.contains_key(&source) {
			self.conceals.insert(source.clone(), ConcealData::default());
		}
		if !self.styles.contains_key(&source) {
			self.styles.insert(source.clone(), StylesData::new());
		}
		if !self.coderanges.contains_key(&source) {
			self.coderanges.insert(source.clone(), CodeRangeData::new());
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

	pub fn with_semantics<'lsp, F, R>(&'lsp self, source: Arc<dyn Source>, f: F) -> Option<R>
		where F: FnOnce(&Semantics, &'lsp Tokens) -> R
	{
		match Semantics::from_source(source, self) {
			Some(sems) => Some(f(&sems, &self.semantic_tokens)),
			None => None,
		}
	}

	pub fn add_hover<'lsp>(&'lsp self, range: Token, content: String) {
		eprintln!("HERE ORIG={range:#?}");
		let Some (hov) = Hover::from_source(range.source(), self) else { return };
		let original = range.source().original_range(range.range);
		hov.add(HoverRange{
			range: original,
			content,
		});
	}
}
