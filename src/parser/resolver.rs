use core::panic;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;

use ariadne::Fmt;

use crate::cache::cache::Cache;
use crate::compiler::compiler::Target;
use crate::compiler::links::get_unique_link;
use crate::compiler::links::translate_reference;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::unit::references::Refname;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;
use crate::unit::unit::OffloadedUnit;
use crate::unit::unit::Reference;

#[derive(Debug)]
pub enum ResolveError {
	InvalidPath(String),
	NotFound(String),
}

pub struct Resolver<'u> {
	/// List of available units via reference_key
	units: HashMap<String, OffloadedUnit<'u>>,
}

/// Dependency of an unit to another unit
#[derive(Debug)]
pub struct UnitDependency {
	/// Depends on this unit for
	pub depends_for: String,
	/// Range where the dependency was introduced
	pub range: Range<usize>,
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
				.is_some()
			{
				panic!("Duplicate units in database");
			}
			Result::<(), ()>::Ok(())
		});

		// Add provided units
		for loaded in provided {
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
		target: Target,
		unit: &TranslationUnit,
		refname: &Refname,
	) -> Result<(String, Reference, String), ResolveError> {
		match refname {
			Refname::Internal(name) => unit
				.get_reference(&name)
				.map(|elem| {
					let reference = Reference {
						refname: name.to_owned(),
						refkey: elem.refcount_key().to_string(),
						source_unit: unit.input_path().to_owned(),
						token: elem.location().range.clone(),
						link: elem.get_link().unwrap().to_owned(),
					};
					(
						translate_reference(
							target,
							&OffloadedUnit::Loaded(unit),
							&OffloadedUnit::Loaded(unit),
							&reference,
						),
						reference,
						unit.reference_key(),
					)
				})
				.ok_or(ResolveError::NotFound(name.clone())),
			Refname::External(path, name) => {
				if !path.is_empty()
				// Query from given unit path
				{
					let provider = self
						.units
						.get(path)
						.ok_or(ResolveError::InvalidPath(path.to_owned()))?;
					provider
						.query_reference(cache.clone(), &name)
						.map(|reference| {
							(
								translate_reference(
									target,
									provider,
									&OffloadedUnit::Loaded(unit),
									&reference,
								),
								reference,
								provider.reference_key(),
							)
						})
						.ok_or(ResolveError::NotFound(refname.to_string()))
				} else
				// Search in all units
				{
					self.units
						.iter()
						.find_map(|(_, unit)| {
							unit.query_reference(cache.clone(), &name)
								.and_then(|reference| Some((reference, unit)))
						})
						.map(|(reference, provider)| {
							(
								translate_reference(
									target,
									provider,
									&OffloadedUnit::Loaded(unit),
									&reference,
								),
								reference,
								provider.reference_key(),
							)
						})
						.ok_or(ResolveError::NotFound(name.to_owned()))
				}
			}
			Refname::Bibliography(path, name) => todo!(),
		}
	}

	/// Resolve links for internal references
	pub fn resolve_links(&self, cache: Arc<Cache>, target: Target) {
		self.units.iter().for_each(|(_, unit)| {
			let OffloadedUnit::Loaded(unit) = unit else {
				return;
			};

			// Used links by this unit
			let mut used_links = HashSet::default();
			unit.references().iter().for_each(|(name, reference)| {
				let link = get_unique_link(target, &mut used_links, name);
				reference.set_link(link);
			});
		});
	}

	fn add_dependency(
		deps: &mut HashMap<String, HashMap<String, Vec<UnitDependency>>>,
		unit: &String,
		depends_on: &String,
		dependency: UnitDependency,
	) {
		if depends_on == unit {
			return;
		}
		match deps.get_mut(unit) {
			Some(map) => match map.get_mut(depends_on) {
				Some(list) => list.push(dependency),
				None => {
					map.insert(depends_on.to_owned(), vec![dependency]);
				}
			},
			None => {
				let mut map = HashMap::new();
				map.insert(depends_on.to_owned(), vec![dependency]);
				deps.insert(unit.to_owned(), map);
			}
		}
	}

	/// Resolves all references and populate reports if required
	pub fn resolve_references(
		&self,
		cache: Arc<Cache>,
		target: Target,
	) -> Result<HashMap<String, HashMap<String, Vec<UnitDependency>>>, Vec<Report>> {
		let mut errors = vec![];
		let mut dependencies = HashMap::new();
		self.units.iter().for_each(|(_, unit)| {
			let OffloadedUnit::Loaded(unit) = unit else {
				return;
			};

			let reference_key = unit.reference_key();
			unit.get_entry_scope()
				.content_iter(true)
				.filter_map(|(scope, elem)| elem.as_linkable().and_then(|link| Some((scope, link))))
				.filter(|(_, elem)| elem.wants_link())
				.for_each(|(_, linkable)| {
					match self.resolve_reference(
						cache.clone(),
						target,
						unit,
						linkable.wants_refname(),
					) {
						// Link reference
						Ok((link, reference, depends_on)) => {
							Self::add_dependency(
								&mut dependencies,
								&reference_key,
								&depends_on,
								UnitDependency {
									depends_for: reference.refname.clone(),
									range: linkable.original_location().range,
								},
							);
							linkable.set_link(reference, link);
						}
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
		if errors.is_empty() {
			Ok(dependencies)
		} else {
			Err(errors)
		}
	}
}
