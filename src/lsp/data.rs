use std::collections::HashMap;
use std::rc::Rc;

use crate::parser::source::Source;

use super::conceal::ConcealsData;
use super::definition::DefinitionData;
use super::hints::HintsData;
use super::semantic::SemanticsData;
use super::semantic::Tokens;

#[derive(Debug)]
pub struct LSPData {
	pub semantic_tokens: Tokens,
	pub semantic_data: HashMap<Rc<dyn Source>, SemanticsData>,
	pub inlay_hints: HashMap<Rc<dyn Source>, HintsData>,
	pub definitions: HashMap<Rc<dyn Source>, DefinitionData>,
	pub conceals: HashMap<Rc<dyn Source>, ConcealsData>,
}

impl LSPData {
	pub fn new() -> Self {
		Self {
			semantic_tokens: Tokens::new(),
			semantic_data: HashMap::new(),
			inlay_hints: HashMap::new(),
			definitions: HashMap::new(),
			conceals: HashMap::new(),
		}
	}

	/// Method that must be called when a source is added
	pub fn new_source(&mut self, source: Rc<dyn Source>) {
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
			self.conceals
				.insert(source.clone(), ConcealsData::new(source.clone()));
		}
	}
}
