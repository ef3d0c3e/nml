use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use parking_lot::RwLock;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::compiler::compiler::Target;
use crate::parser::source::Token;
use crate::unit::scope::Scope;
use crate::unit::translation::{CustomData, TranslationUnit};

pub static TAGGED_CUSTOM: &str = "nml.tagged.registered";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaggedKind {
	/// Process raw content ranges
	Raw,
	/// Process parsed content
	Parsed,
}

impl TryFrom<&str> for TaggedKind {
	type Error = String;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		match value {
			"raw" => Ok(TaggedKind::Raw),
			"parsed" => Ok(TaggedKind::Parsed),
			_ => Err(format!("Invalid TaggedKind, expected `raw' or `parsed'")),
		}
	}
}

pub enum TaggedClosure {
	Raw(
		Arc<
			dyn Fn(&mut TranslationUnit, Token, Vec<Range<usize>>) -> mlua::Result<()>
				+ Send
				+ Sync,
		>,
	),
	Parsed(
		Arc<
			dyn Fn(&mut TranslationUnit, Token, Vec<Arc<RwLock<Scope>>>) -> mlua::Result<()>
				+ Send
				+ Sync,
		>,
	),
}

pub struct TaggedProcessor {
	pub kind: TaggedKind,
	pub closure: TaggedClosure,
}

/// Data for tagged content
#[derive(Default)]
pub struct TaggedData {
	/// All registered tagged processor
	pub(crate) registered: HashMap<String, Arc<TaggedProcessor>>,
}

impl CustomData for TaggedData {
	fn name(&self) -> &str {
		TAGGED_CUSTOM
	}
}

impl TaggedData {
	/// Add a tagged processor
	pub fn add_processor(unit: &mut TranslationUnit, name: String, processor: TaggedProcessor) {
		if !unit.has_data(TAGGED_CUSTOM) {
			unit.new_data(Arc::new(RwLock::new(TaggedData::default())));
		}

		unit.with_data::<TaggedData, _, _>(TAGGED_CUSTOM, |mut data| {
			data.registered.insert(name, Arc::new(processor));
		});
	}

	pub fn get_processor(unit: &mut TranslationUnit, name: &str) -> Option<Arc<TaggedProcessor>> {
		if !unit.has_data(TAGGED_CUSTOM) {
			unit.new_data(Arc::new(RwLock::new(TaggedData::default())));
		}

		unit.with_data::<TaggedData, _, _>(TAGGED_CUSTOM, |data| data.registered.get(name).cloned())
	}
}
