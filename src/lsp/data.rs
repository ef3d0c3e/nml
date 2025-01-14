use std::collections::HashMap;
use std::sync::Arc;

use crate::parser::source::Source;

use super::code::CodeRangeData;
use super::conceal::ConcealsData;
use super::definition::DefinitionData;
use super::hints::HintsData;
use super::semantic::SemanticsData;
use super::semantic::Tokens;
use super::styles::StylesData;

#[derive(Debug)]
pub struct LSPData {
	pub semantic_tokens: Tokens,
	pub semantic_data: HashMap<Arc<dyn Source>, SemanticsData>,
	pub inlay_hints: HashMap<Arc<dyn Source>, HintsData>,
	pub definitions: HashMap<Arc<dyn Source>, DefinitionData>,
	pub conceals: HashMap<Arc<dyn Source>, ConcealsData>,
	pub styles: HashMap<Arc<dyn Source>, StylesData>,
	pub coderanges: HashMap<Arc<dyn Source>, CodeRangeData>,
}

impl LSPData {
	pub fn new() -> Self {
		Self {
			semantic_tokens: Tokens::new(),
			semantic_data: HashMap::new(),
			inlay_hints: HashMap::new(),
			definitions: HashMap::new(),
			conceals: HashMap::new(),
			styles: HashMap::new(),
			coderanges: HashMap::new(),
		}
	}

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
}
