use nml_document::Export;
use nml_document::LinkSlot;
use nml_document::Property;
use nml_document::Scope;
use nml_document::SymbolRef;
use nml_lang::relative_url;
use nml_workspace::Index;
use nml_workspace::IndexError;
use nml_workspace::Role;
use nml_workspace::UnitRecord;

/// Why a reference does not resolve.
#[derive(Debug)]
pub(crate) enum LinkError {
	Unresolved,
	/// So many units match.
	Ambiguous(usize),
	Index(IndexError),
}

impl From<IndexError> for LinkError {
	fn from(error: IndexError) -> Self {
		Self::Index(error)
	}
}

/// What a reference resolves to: a unit, and a symbol of it unless it is a page reference.
struct Target {
	unit: UnitRecord,
	export: Option<Export>,
}

/// Resolves the references of one unit against the index.
pub(crate) struct Linker<'a> {
	pub index: &'a Index,
	pub unit: &'a UnitRecord,
}

impl Linker<'_> {
	fn resolve(&self, target: &SymbolRef) -> Result<Target, LinkError> {
		let export = |unit: UnitRecord| -> Result<Target, LinkError> {
			if target.is_page() {
				return Ok(Target { unit, export: None });
			}
			match self.index.export(unit.id, &target.name)? {
				Some(export) => Ok(Target {
					unit,
					export: Some(export),
				}),
				None => Err(LinkError::Unresolved),
			}
		};
		match &target.scope {
			Scope::Local => export(self.unit.clone()),
			Scope::Unit(name) => {
				let mut units = self.index.units_named(name)?;
				match units.len() {
					0 => Err(LinkError::Unresolved),
					1 => export(units.pop().unwrap()),
					count => Err(LinkError::Ambiguous(count)),
				}
			}
			Scope::Any => {
				let exporters = self.index.exporters(&target.name)?;
				match exporters.as_slice() {
					[] => Err(LinkError::Unresolved),
					[unit] => {
						let unit = self.index.unit_by_id(*unit)?.unwrap();
						export(unit)
					}
					units => Err(LinkError::Ambiguous(units.len())),
				}
			}
		}
	}

	/// The value of a link slot.
	pub fn value(&self, link: &LinkSlot) -> Result<String, LinkError> {
		let target = self.resolve(&link.target)?;
		let property = |name: &str| {
			target
				.export
				.as_ref()
				.and_then(|export| export.property(name))
				.map(String::from)
		};
		Ok(match &link.property {
			Property::Target => {
				if target.unit.role != Role::Page {
					return Err(LinkError::Unresolved);
				}
				let mut url = if target.unit.id == self.unit.id && target.export.is_some() {
					String::new()
				} else {
					relative_url(&self.unit.path, &target.unit.path)
				};
				if let Some(export) = &target.export {
					url.push('#');
					url.push_str(export.property("anchor").unwrap_or(&export.name));
				}
				url
			}
			Property::Title => match &target.export {
				Some(export) => property("title").unwrap_or_else(|| export.name.clone()),
				None => target.unit.title.clone(),
			},
			Property::Number => property("number").unwrap_or_default(),
			Property::Other(name) => property(name).unwrap_or_default(),
			_ => String::new(),
		})
	}
}
