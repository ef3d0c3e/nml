use std::{borrow::Borrow, collections::HashMap, sync::{Arc, MutexGuard}};

use rusqlite::Connection;
use url::Url;

use crate::{document::references::Refname, parser::{scope::ScopeAccessor, source::Source, translation::TranslationUnit}};

use super::compiler::Target;

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
	references: HashMap<Refname, Vec<ErasedReference>>,
}

impl<'u> Resolver<'u> {
	pub fn add_unit(&mut self, unit: &'u TranslationUnit<'u>, target: Target) {
		todo!();
		for (_, elem) in unit.get_entry_scope().content_iter()
		{
			let source : Arc<dyn Source> = unit.get_entry_scope().borrow().source();
			if let Some(referenceable) = elem.as_referenceable()
			{
				//let erased = ErasedReference {
				//	source: source.name().into(),
				//	name: referenceable.reference_name().cloned(),
				//	url: referenceable.get_url(target),
				//};
				//referenceable.reference_name(target);
			}
		}
	}

	pub fn import_from_cache<'con>(&mut self, mut con: MutexGuard<'con, Connection>) {
		todo!();
	}
}
