use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;

use crate::cache::cache::Cache;

use super::translation::TranslationAccessors;
use super::translation::TranslationUnit;

/// Link/Compile-time reference
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
	/// Name of reference
	pub refname: String,
	/// Type of reference
	pub refkey: String,
	/// Source unit path, relative to the database
	pub source_unit: PathBuf,
	/// The reference link in it's own unit
	pub link: String,
	/// Declaring token of the reference
	pub token: Range<usize>,
}

/// In-database translation unit
pub struct DatabaseUnit {
	pub reference_key: String,
	pub input_file: PathBuf,
	pub output_file: Option<PathBuf>,
}

/// Wrapper units that may be present in memory or in the database
pub enum OffloadedUnit<'u> {
	/// In-memory translation unit
	Loaded(&'u TranslationUnit),
	/// In-database translation unit
	Unloaded(DatabaseUnit),
}

impl<'u> OffloadedUnit<'u> {
	/// Gets the unit's reference key
	pub fn reference_key(&self) -> String {
		match self {
			OffloadedUnit::Loaded(unit) => unit.reference_key(),
			OffloadedUnit::Unloaded(unit) => unit.reference_key.clone(),
		}
	}

	/// Gets the unit's input path
	pub fn input_path(&self) -> &PathBuf {
		match self {
			OffloadedUnit::Loaded(unit) => unit.input_path(),
			OffloadedUnit::Unloaded(unit) => &unit.input_file,
		}
	}

	/// Gets the unit's output path
	pub fn output_path(&self) -> &PathBuf {
		match self {
			OffloadedUnit::Loaded(unit) => unit.output_path().as_ref().unwrap(),
			OffloadedUnit::Unloaded(unit) => unit.output_file.as_ref().unwrap(),
		}
	}

	/// Find reference named `name` in the unit
	/// This returns an owned value.
	pub fn query_reference<S: AsRef<str>>(&self, cache: Arc<Cache>, name: S) -> Option<Reference> {
		match self {
			OffloadedUnit::Loaded(unit) => unit.get_reference(&name).map(|elem| Reference {
				refname: name.as_ref().to_string(),
				refkey: elem.refcount_key().to_string(),
				source_unit: unit.input_path().to_owned(),
				token: elem.location().range.clone(),
				link: elem.get_link().unwrap().to_owned(),
			}),
			OffloadedUnit::Unloaded(unit) => cache.query_reference(unit, name.as_ref()),
		}
	}
}
