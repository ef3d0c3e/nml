use std::collections::HashMap;
use std::sync::Arc;

use crate::parser::source::Source;
use crate::parser::source::SourceFile;
use crate::parser::source::SourcePosition;
use crate::parser::source::VirtualSource;

use super::code::CodeRangeData;
use super::conceal::ConcealsData;
use super::definition::DefinitionData;
use super::hints::HintsData;
use super::semantic::Semantics;
use super::semantic::SemanticsData;
use super::semantic::Tokens;
use super::styles::StylesData;

/// Stores data for a translation unit that will be passed to the language server
#[derive(Debug, Default)]
pub struct LangServerData {
	/// Available semantic tokens
	pub semantic_tokens: Tokens,
	/// List of semantic tokens for this translatiop unit
	pub semantic_data: HashMap<Arc<dyn Source>, SemanticsData>,
	pub inlay_hints: HashMap<Arc<dyn Source>, HintsData>,
	pub definitions: HashMap<Arc<dyn Source>, DefinitionData>,
	pub conceals: HashMap<Arc<dyn Source>, ConcealsData>,
	pub styles: HashMap<Arc<dyn Source>, StylesData>,
	pub coderanges: HashMap<Arc<dyn Source>, CodeRangeData>,
}

impl LangServerData {
	/// Method that must be called when a source is added
	pub fn new_source(&mut self, source: Arc<dyn Source>) {
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
		if !self.conceals.contains_key(&source) {
			self.conceals.insert(source.clone(), ConcealsData::new());
		}
		if !self.styles.contains_key(&source) {
			self.styles.insert(source.clone(), StylesData::new());
		}
		if !self.coderanges.contains_key(&source) {
			self.coderanges.insert(source.clone(), CodeRangeData::new());
		}
	}

	fn get_original_source(source: Arc<dyn Source>) -> Option<Arc<dyn Source>>
	{
		// TODO: This should be refactored
		if (source.name().starts_with(":LUA:") || source.name().starts_with(":VAR:"))
			&& source.downcast_ref::<VirtualSource>().is_some()
		{
			return None;
		}

		if let Some(location) = source
			.clone()
			.downcast_ref::<VirtualSource>()
			.map(|parent| parent.location())
			.unwrap_or(None)
		{
			return Self::get_original_source(location.source());
		} else if source.downcast_ref::<SourceFile>().is_some() {
			return Some(source)
		}
		None
	}

	//pub fn on_scope_end(&mut self, source: Arc<dyn Source>) {
	//	if source.content().is_empty() {
	//		return;
	//	}
	//	// Process the rest of the semantic queue for the current source
	//	let pos = source.original_position(source.content().len() - 1).1;
	//	if let Some((sems, _)) = Semantics::from_source(source, lsp) {
	//		sems.process_queue(pos);
	//	}
	//}

	pub fn with_semantics<'lsp, F, R>(&'lsp self, source: Arc<dyn Source>, f: F) -> Option<R>
		where F: FnOnce(&Semantics, &'lsp Tokens) -> R
	{
		match Semantics::from_source(source, self) {
			Some(sems) => Some(f(&sems, &self.semantic_tokens)),
			None => None,
		}
	}
}
