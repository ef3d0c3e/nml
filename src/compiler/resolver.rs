use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::MutexGuard;

use rusqlite::Connection;
use url::Url;

use crate::document::references::Refname;

use super::compiled::CompiledUnit;

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
	_pd: PhantomData<&'u i32>,
}

/// Stores the result of resolved data
/// Every reference asked by an element should be present in this structure
#[derive(Default)]
pub struct ResolveData {
	references: HashMap<Refname, Vec<ErasedReference>>,
}

impl<'u> Resolver<'u> {
	pub fn resolve<'con>(
		&self,
		units: Vec<CompiledUnit>,
		mut con: MutexGuard<'con, Connection>,
	) -> ResolveData {
		ResolveData::default()
	}
}
