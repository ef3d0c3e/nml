use std::{cell::RefCell, collections::HashMap, ops::Range, path::PathBuf, rc::Rc, sync::{Arc, MutexGuard}};

use rusqlite::Connection;

use crate::document::{element::{Element, ReferenceableElement}, references::{InternalReference, Refname}};

use super::{scope::{Scope, ScopeAccessor}, source::Source, translation::{TranslationAccessors, TranslationUnit}};

pub struct Resolver<'con, 'u>
{
	pub con: MutexGuard<'con, Connection>,
	pub units: Vec<&'u TranslationUnit<'u>>,
}

/// Link/Compile-time reference
#[derive(Debug)]
pub struct Reference
{
	/// Name of reference
	pub refname: Refname,
	/// Type of reference
	pub refkey: String,
	/// Definition unit of this reference
	pub source_unit: PathBuf,
	/// Declaring token of the reference
	pub token: Range<usize>,
	/// [FIXME] anchor to the reference
	/// This anchor should be made compiler agnostic...
	pub anchor: String,
}

#[derive(Debug)]
pub enum ResolveError
{
	NotFound(String),
}

impl<'con, 'u> Resolver<'con, 'u>
{
	pub fn resolve_reference(&self, unit: &TranslationUnit, refname: &Refname) -> Option<Reference>
	{
		match refname {
			Refname::Internal(name) =>
				unit.get_reference(refname)
					.map(|elem| Reference {
						refname: refname.clone(),
						refkey: elem.refcount_key().to_string(),
						source_unit: unit.get_path().clone(),
						token: elem.location().range.clone(),
						anchor: todo!(),
					}),
			Refname::External(path, name) => {
				todo!();
			},
			Refname::Bibliography(path, name) => todo!(),
		}
	}
}
