use std::collections::HashMap;

use ariadne::Fmt;
use rusqlite::Connection;
use tokio::sync::MutexGuard;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::unit::database::DatabaseUnit;
use crate::unit::references::Refname;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::{TranslationAccessors, TranslationUnit};
use crate::unit::unit::{OffloadedUnit, Reference};

#[derive(Debug)]
pub enum ResolveError
{
	InvalidPath(String),
	NotFound(String),
}

pub struct Resolver<'u>
{
	/// List of available units via reference_key
	units: HashMap<String, OffloadedUnit<'u>>
}


impl<'u> Resolver<'u>
{
	pub fn new<'con>(colors: &ReportColors, con: &MutexGuard<'con, Connection>, provided: &'u Vec<TranslationUnit<'u>>) -> Result<Self, Report>
	{
		// Init tables
		con.execute(
			"CREATE TABLE IF NOT EXISTS referenceable_units(
				reference_key	TEXT PRIMARY KEY,
				input_file		TEXT NOT NULL,
				output_file		TEXT NOT NULL
			);", ()).unwrap();
		con.execute(
			"CREATE TABLE IF NOT EXISTS exported_references(
				name			TEXT PRIMARY KEY,
				data			TEXT NOT NULL,
				unit			TEXT NOT NULL,
				FOREIGN KEY(unit) REFERENCES referenceable_units(reference_key)
			);", ()).unwrap();
		println!("HERE!");

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
			if let Some(previous) = units.insert(unloaded.0.clone(), OffloadedUnit::Unloaded(DatabaseUnit {
				reference_key: unloaded.0.clone(),
				input_file: unloaded.1.clone(),
				output_file: unloaded.2
			})) {
				panic!("Duplicate unit in database")
				// Should not happen since the database should enforce uniqueness
			}
		}

		// Add provided units
		for loaded in provided
		{
			match units.insert(loaded.reference_key().to_owned(), OffloadedUnit::Loaded(
				loaded)).as_ref()
			{
				Some(OffloadedUnit::Unloaded(previous)) =>
				{
					// Duplicate with a database unit
					if previous.input_file != *loaded.input_path()
					{
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
								format!("Unit 1: `{}` with key `{}`", loaded.input_path().fg(colors.info), loaded.reference_key().fg(colors.info))
							),
							note(format!("Unit 2: `{}` with key `{}`", (&previous.input_file).fg(colors.info), (&previous.reference_key).fg(colors.info)))
						))?;
					}
				},
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
								format!("Unit 1: `{}` with key `{}`", loaded.input_path().fg(colors.info), loaded.reference_key().fg(colors.info))
							),
							span_info(
								previous.token().source(),
								0..0,
								format!("Unit 2: `{}` with key `{}`", previous.input_path().fg(colors.info), previous.reference_key().fg(colors.info))
							),
					))?;
				},
				_ => {},
			}
		}

		Ok(Self{
			units,
		})
	}

	pub fn resolve_reference<'con>(&self, con: &MutexGuard<'con, Connection>, unit: &TranslationUnit, refname: &Refname) -> Result<Reference, ResolveError>
	{
		match refname {
			Refname::Internal(name) =>
				unit.get_reference(&name)
					.map(|elem| Reference {
						refname: name.to_owned(),
						refkey: elem.refcount_key().to_string(),
						source_unit: unit.input_path().to_owned(),
						token: elem.location().range.clone(),
					})
			.ok_or(ResolveError::NotFound(name.clone())),
			Refname::External(path, name) => {
				println!("Resolve: {path:#?}");
				println!("Resolve: {:#?}", unit.reference_key());
				let provider = self.units.get(path).ok_or(
					ResolveError::InvalidPath(path.to_owned())
				)?;
				provider.query_reference(&con, &name).ok_or(
					ResolveError::NotFound(refname.to_string()))
			},
			Refname::Bibliography(path, name) => todo!(),
		}
	}

	/// Resolves all references and populate reports if required
	pub fn resolve_all<'con>(&self, con: &MutexGuard<'con, Connection>) -> Vec<Report> {
		let mut errors = vec![];
		self.units
			.iter()
			.for_each(|(_, unit)| {
				let OffloadedUnit::Loaded(unit) = unit else { return };

				unit.get_entry_scope()
					.content_iter()
					.filter_map(|(_, elem)| elem.as_linkable())
					.filter(|elem| elem.wants_link())
					.for_each(|linkable| {
						println!("RESOLV={:#?}", linkable.wants_refname());
						match self.resolve_reference(&con, unit, linkable.wants_refname())
						{
							/// Link reference
							Ok(link) => { println!("resolved to: {link:#?}"); linkable.link(link) },
							Err(ResolveError::InvalidPath(path)) => {
								errors.push(
									make_err!(
										linkable.location().source(),
										"Linking failed".into(),
										span(
											linkable.location().range.clone(),
											format!("Failed to resolve `{}`: Reference path `{}` not found",
												linkable.wants_refname().to_string().fg(unit.colors().info),
												path.fg(unit.colors().info))
										)
									));
							},
							Err(ResolveError::NotFound(name)) => {
								errors.push(
									make_err!(
										linkable.location().source(),
										"Linking failed".into(),
										span(
											linkable.location().range.clone(),
											format!("Failed to resolve `{}`: Reference not found",
												name.fg(unit.colors().info))
										)
									));
							}
						}
					});
			});
		errors
	}
}
