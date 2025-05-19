use core::panic;
use std::collections::HashMap;
use std::sync::Arc;

use ariadne::Fmt;
use rusqlite::Connection;
use tokio::sync::MutexGuard;
use tower_lsp::lsp_types::WorkspaceFileOperationsServerCapabilities;

use crate::cache::cache::Cache;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::unit::references::Refname;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::{TranslationAccessors, TranslationUnit};
use crate::unit::unit::{DatabaseUnit, OffloadedUnit, Reference};

#[derive(Debug)]
pub enum ResolveError {
	InvalidPath(String),
	NotFound(String),
}

pub struct Resolver<'u> {
	/// List of available units via reference_key
	units: HashMap<String, OffloadedUnit<'u>>,
}

impl<'u> Resolver<'u> {
	pub fn new(
		colors: &ReportColors,
		cache: Arc<Cache>,
		provided: &'u Vec<TranslationUnit<'u>>,
	) -> Result<Self, Report> {
		let mut units = HashMap::default();

		cache.load_units(|unit| {
			if units
				.insert(unit.reference_key.clone(), OffloadedUnit::Unloaded(unit))
				.is_some() {
				panic!("Duplicate units in database");
			}
			Result::<(), ()>::Ok(())
		});

		// Add provided units
		for loaded in provided {
			println!("loaded.input_path={:#?}", loaded.input_path());
			match units
				.insert(
					loaded.reference_key().to_owned(),
					OffloadedUnit::Loaded(loaded),
				)
				.as_ref()
			{
				Some(OffloadedUnit::Unloaded(previous)) => {
					// Duplicate with a database unit
					if previous.input_file != *loaded.input_path() {
						let token = loaded.token();
						Err(make_err!(
							token.source(),
							"Unable to resolve project".into(),
							span(
								0..0,
								format!("Two units are sharing the same reference key!")
							),
							span_info(
								0..0,
								format!(
									"Unit 1: `{}` with key `{}`",
									loaded.input_path().fg(colors.info),
									loaded.reference_key().fg(colors.info)
								)
							),
							note(format!(
								"Unit 2: `{}` with key `{}`",
								(&previous.input_file).fg(colors.info),
								(&previous.reference_key).fg(colors.info)
							))
						))?;
					}
				}
				Some(OffloadedUnit::Loaded(previous)) => {
					// Duplicate within parameters
					Err(make_err!(
						loaded.token().source(),
						"Unable to resolve project".into(),
						span(
							0..0,
							format!("Two units are sharing the same reference key!")
						),
						span_info(
							0..0,
							format!(
								"Unit 1: `{}` with key `{}`",
								loaded.input_path().fg(colors.info),
								loaded.reference_key().fg(colors.info)
							)
						),
						span_info(
							previous.token().source(),
							0..0,
							format!(
								"Unit 2: `{}` with key `{}`",
								previous.input_path().fg(colors.info),
								previous.reference_key().fg(colors.info)
							)
						),
					))?;
				}
				_ => {}
			}
		}

		Ok(Self { units })
	}

	/// Resolvers a single reference
	pub fn resolve_reference(
		&self,
		cache: Arc<Cache>,
		unit: &TranslationUnit,
		refname: &Refname,
	) -> Result<Reference, ResolveError> {
		match refname {
			Refname::Internal(name) => unit
				.get_reference(&name)
				.map(|elem| Reference {
					refname: name.to_owned(),
					refkey: elem.refcount_key().to_string(),
					source_unit: unit.input_path().to_owned(),
					token: elem.location().range.clone(),
				})
				.ok_or(ResolveError::NotFound(name.clone())),
			Refname::External(path, name) => {
				if !path.is_empty()
				// Query from give unit path
				{
					let provider = self
						.units
						.get(path)
						.ok_or(ResolveError::InvalidPath(path.to_owned()))?;
					provider
						.query_reference(cache.clone(), &name)
						.ok_or(ResolveError::NotFound(refname.to_string()))
				} else
				// Search in all units
				{
					self.units
						.iter()
						.find_map(|(_, unit)| unit.query_reference(cache.clone(), &name))
						.ok_or(ResolveError::NotFound(name.to_owned()))
				}
			}
			Refname::Bibliography(path, name) => todo!(),
		}
	}

	/// Resolves all references and populate reports if required
	pub fn resolve_all(&self, cache: Arc<Cache>) -> Vec<Report> {
		let mut errors = vec![];
		self.units.iter().for_each(|(_, unit)| {
			let OffloadedUnit::Loaded(unit) = unit else {
				return;
			};

			unit.get_entry_scope()
				.content_iter(true)
				.filter_map(|(scope, elem)| elem.as_linkable().and_then(|link| Some((scope, link))))
				.filter(|(_, elem)| elem.wants_link())
				.for_each(|(scope, linkable)| {
					match self.resolve_reference(cache.clone(), unit, linkable.wants_refname()) {
						// Link reference
						Ok(link) => linkable.link(link),
						Err(ResolveError::InvalidPath(path)) => {
							errors.push(make_err!(
								linkable.location().source(),
								"Linking failed".into(),
								span(
									linkable.location().range.clone(),
									format!(
										"Failed to resolve `{}`: Reference path `{}` not found",
										linkable.wants_refname().to_string().fg(unit.colors().info),
										path.fg(unit.colors().info)
									)
								)
							));
						}
						Err(ResolveError::NotFound(name)) => {
							errors.push(make_err!(
								linkable.location().source(),
								"Linking failed".into(),
								span(
									linkable.location().range.clone(),
									format!(
										"Failed to resolve `{}`: Reference not found",
										name.fg(unit.colors().info)
									)
								)
							));
						}
					}
				});
		});
		errors
	}
}
