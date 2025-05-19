use std::{ops::Range, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::cache::cache::Cache;

use super::{translation::{TranslationAccessors, TranslationUnit}};

/// Link/Compile-time reference
#[derive(Debug, Serialize, Deserialize)]
pub struct Reference
{
	/// Name of reference
	pub refname: String,
	/// Type of reference
	pub refkey: String,
	/// Source unit path, relative to the database
	pub source_unit: String,
	/// The reference link in it's own unit
	pub link: String,
	/// Declaring token of the reference
	pub token: Range<usize>,
}

/// In-database translation unit
pub struct DatabaseUnit
{
	pub reference_key: String,
	pub input_file: String,
	pub output_file: Option<String>,
}

/// Wrapper units that may be present in memory or in the database
pub enum OffloadedUnit<'u>
{
	/// In-memory translation unit
	Loaded(&'u TranslationUnit<'u>),
	/// In-database translation unit
	Unloaded(DatabaseUnit),
}

impl<'u> OffloadedUnit<'u> {
	/// Gets the unit's reference key
	pub fn reference_key(&self) -> String
	{
		match self {
			OffloadedUnit::Loaded(unit) => unit.reference_key(),
			OffloadedUnit::Unloaded(unit) => unit.reference_key.clone(),
		}
	}

	/// Gets the unit's input path
	pub fn input_path(&self) -> String {
		match self {
			OffloadedUnit::Loaded(unit) => unit.input_path().to_owned(),
			OffloadedUnit::Unloaded(unit) => unit.input_file.clone(),
		}
	}

	/// Find reference named `name` in the unit
	/// This returns an owned value.
	pub fn query_reference<S: AsRef<str>>(&self, cache: Arc<Cache>, name: S) -> Option<Reference>
	{
		match self {
			OffloadedUnit::Loaded(unit) => {
				unit.get_reference(&name)
					.map(|elem| Reference {
						refname: name.as_ref().to_string(),
						refkey: elem.refcount_key().to_string(),
						source_unit: unit.input_path().to_owned(),
						token: elem.location().range.clone(),
						link: elem.get_link().unwrap().to_owned(),
					})
			},
			OffloadedUnit::Unloaded(unit) => cache.query_reference(unit, name.as_ref())
		}
	}
}
