use std::{cell::RefCell, collections::HashMap, ops::Range, path::{Path, PathBuf}, rc::Rc, sync::{Arc}};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tokio::sync::MutexGuard;

use crate::document::{element::{Element, ReferenceableElement}, references::{InternalReference, Refname}};

use super::{scope::{Scope, ScopeAccessor}, source::Source, translation::{TranslationAccessors, TranslationUnit}};

/// Link/Compile-time reference
#[derive(Debug, Serialize, Deserialize)]
pub struct Reference
{
	/// Name of reference
	pub refname: Refname,
	/// Type of reference
	pub refkey: String,
	/// Source unit path, relative to the database
	pub source_unit: String,
	/// Declaring token of the reference
	pub token: Range<usize>,
	/// [FIXME] anchor to the reference
	/// This anchor should be made compiler agnostic...
	pub anchor: String,
}

pub enum ResolverUnit<'u>
{
	Unloaded(String, String, String),
	Loaded(&'u TranslationUnit<'u>),
}

#[derive(Debug)]
pub enum ResolveError
{
	NotFound(String),
	InvalidPath(String),
}

pub struct Resolver<'u>
{
	units: HashMap<String, ResolverUnit<'u>>
}


impl<'u> Resolver<'u>
{
	pub fn new<'con>(con: MutexGuard<'con, Connection>, provided: &'u Vec<TranslationUnit<'u>>) -> Result<Self, String>
	{
		// Init tables
		con.execute(
			"CREATE TABLE IF NOT EXISTS referenceable_units(
				reference_key	TEXT PRIMARY KEY,
				input_file		TEXT NOT NULL,
				output_file		TEXT NOT NULL
			);
			CREATE TABLE IF NOT EXISTS references(
				FOREIGN KEY(unit) REFERENCES referenceable_unit(reference_key),
				name			TEXT PRIMARY KEY,
				data			TEXT NOT NULL
			);", ()
		).unwrap();

		let mut units = HashMap::default();

		// Load from database
		let mut cmd = con.prepare("SELECT * FROM referenceable_units").unwrap();
		let unlodaded_iter = cmd.query_map([], |row| {
			Ok((row.get(0).unwrap(),
				row.get(1).unwrap(),
				row.get(2).unwrap()))
		}).unwrap();
		for unloaded in unlodaded_iter
		{
			let unloaded : (String, String, String) = unloaded.unwrap();
			if let Some(ResolverUnit::Unloaded(previous_key, previous_input, _)) = units.insert(unloaded.0.clone(), ResolverUnit::Unloaded(
				unloaded.0.clone(),
				unloaded.1.clone(),
				unloaded.2)) {
				return Err(format!("Duplicate reference key! Unit `{}` [key={}] and unit `{}` [key={}]", unloaded.1, unloaded.0, previous_input, previous_key))
			}
		}

		// Add provided units
		for loaded in provided
		{
			if let Some(ResolverUnit::Unloaded(previous_key, previous_input, _)) = units.insert(loaded.get_refkey().to_owned(), ResolverUnit::Loaded(
				loaded)) {
				if previous_input != *loaded.get_path()
				{
					return Err(format!("Duplicate reference key! Unit `{}` [key={}] and unit `{}` [key={}]", loaded.get_path(), loaded.get_refkey(), previous_input, previous_key))
				}
			}
		}

		Ok(Self{
			units,
		})
	}

	pub fn resolve_reference<'con>(&self, con: MutexGuard<'con, Connection>, unit: &TranslationUnit, refname: &Refname) -> Result<Reference, ResolveError>
	{
		match refname {
			Refname::Internal(name) =>
				unit.get_reference(refname)
					.map(|elem| Reference {
						refname: refname.clone(),
						refkey: elem.refcount_key().to_string(),
						source_unit: unit.get_path().to_owned(),
						token: elem.location().range.clone(),
						anchor: todo!(),
					})
			.ok_or(ResolveError::NotFound(name.clone())),
			Refname::External(path, name) => {
				println!("Resolve: {path:#?}");
				todo!();
			},
			Refname::Bibliography(path, name) => todo!(),
		}
	}
}
