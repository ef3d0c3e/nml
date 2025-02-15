use std::{collections::HashMap, sync::MutexGuard};

use rusqlite::Connection;
use url::Url;

use crate::{document::references::Refname, parser::{scope::ScopeAccessor, translation::TranslationUnit}};

pub struct ErasedReference {
	/// Reference source file (e.g input file)
	source: String,
	/// Reference name (used to display reference)
	name: Option<String>,
	/// Reference URL (for linking purposes)
	url: Url,
}

#[derive(Default)]
pub struct Resolver<'u> {
	references: HashMap<Refname, ErasedReference>,
}

impl<'u> Resolver<'u> {
	pub fn add_unit(&mut self, unit: &'u TranslationUnit<'u>) {
		for (_, elem) in unit.get_entry_scope().content_iter()
		{
			if let Some(referenceable) = elem.as_referenceable()
			{
			}
		}
	}

	pub fn import_from_cache<'con>(&mut self, mut con: MutexGuard<'con, Connection>) {
		self.units.push(unit);
	}

	fn resolve_reference<'con>(&self, name: &Refname) -> Option<ErasedReference> {
		None
	}
}
