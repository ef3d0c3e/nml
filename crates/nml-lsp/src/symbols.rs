//! `workspace/symbol`: the units of the workspace, what they declare for references, and
//! the variables they export, for pickers. From the index, with the open documents as
//! they are being edited.

use std::collections::BTreeMap;

use lsp_types::SymbolInformation;
use lsp_types::SymbolKind as LspKind;
use nml_document::Export;
use nml_document::SymbolKind;

use crate::Result;
use crate::Server;

/// A unit, as symbols see it.
struct UnitSymbols {
	/// Its reference key: `doc` in `[[doc#name]]`.
	key: String,
	title: String,
	exports: Vec<Export>,
}

/// The protocol's kind of a symbol. Clients show them with their own names and icons;
/// [`kind_name`] gives NML's.
fn lsp_kind(kind: SymbolKind) -> LspKind {
	match kind {
		SymbolKind::Heading => LspKind::STRING,
		SymbolKind::Figure | SymbolKind::Media => LspKind::OBJECT,
		SymbolKind::Table => LspKind::ARRAY,
		SymbolKind::Listing => LspKind::STRUCT,
		SymbolKind::Anchor => LspKind::KEY,
		SymbolKind::Variable => LspKind::VARIABLE,
		// The enumeration is not exhaustive.
		_ => LspKind::OBJECT,
	}
}

/// Whether `query`'s characters are in `text` in order, ignoring case: what pickers send
/// is matched loosely, and they sort it themselves.
fn matches(text: &str, query: &str) -> bool {
	let mut text = text.chars().flat_map(char::to_lowercase);
	query
		.chars()
		.flat_map(char::to_lowercase)
		.filter(|c| !c.is_whitespace())
		.all(|wanted| text.any(|c| c == wanted))
}

/// What a symbol is called in a picker: how it is written, and its title.
fn symbol_name(key: &str, export: &Export) -> String {
	if export.kind == SymbolKind::Variable {
		let value = export.property("value").unwrap_or_default();
		return format!("{key}.{} = {value}", export.name);
	}
	let reference = format!("{key}#{}", export.name);
	match export.property("title").filter(|title| !title.is_empty()) {
		Some(title) => format!("{title} ({reference})"),
		None => reference,
	}
}

impl Server {
	/// Every unit of the workspace, by path: from the index, and the open documents' last
	/// analysis instead of their unit in the index.
	fn unit_symbols(&self) -> BTreeMap<String, UnitSymbols> {
		let mut units = BTreeMap::new();
		if let Some(index) = self.index()
			&& let Ok(records) = index.units()
		{
			for unit in records {
				let exports = index.exports(unit.id).unwrap_or_default();
				let symbols = UnitSymbols {
					key: unit.name,
					title: unit.title,
					exports,
				};
				units.insert(unit.path, symbols);
			}
		}
		for analysis in self.analyses.values() {
			// Files outside the workspace, and included files analysed in their includer,
			// have no unit of their own.
			if self.root.is_none() || analysis.context.is_some() {
				continue;
			}
			let title = (units.get(&analysis.path))
				.map_or_else(|| analysis.key.clone(), |unit| unit.title.clone());
			let symbols = UnitSymbols {
				key: analysis.key.clone(),
				title,
				exports: analysis.exports.clone(),
			};
			units.insert(analysis.path.clone(), symbols);
		}
		units
	}

	/// The symbols whose name matches `query`: files first, then by unit.
	#[allow(deprecated)] // `SymbolInformation::deprecated`, which must be set.
	pub(crate) fn workspace_symbols(&self, query: &str) -> Result<Vec<SymbolInformation>> {
		let mut symbols = Vec::new();
		let mut declared = Vec::new();
		for (path, unit) in self.unit_symbols() {
			let Some(file) = self.location(&path, 0..0) else {
				continue;
			};
			let name = format!("{path} ({})", unit.title);
			if matches(&name, query) {
				symbols.push(SymbolInformation {
					name,
					kind: LspKind::FILE,
					tags: None,
					deprecated: None,
					location: file.clone(),
					container_name: Some(unit.key.clone()),
				});
			}
			for export in &unit.exports {
				let name = symbol_name(&unit.key, export);
				if !matches(&name, query) {
					continue;
				}
				// Where it is declared, which can be an included file.
				let location = (export.declared.as_ref())
					.and_then(|(path, range)| self.location(path, range.clone()))
					.unwrap_or_else(|| file.clone());
				declared.push(SymbolInformation {
					name,
					kind: lsp_kind(export.kind),
					tags: None,
					deprecated: None,
					location,
					container_name: Some(unit.key.clone()),
				});
			}
		}
		symbols.extend(declared);
		Ok(symbols)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn loose_matches() {
		assert!(matches("Getting started (index#start)", ""));
		assert!(matches("Getting started (index#start)", "gs"));
		assert!(matches("Getting started (index#start)", "INDEX start"));
		assert!(!matches("Getting started (index#start)", "zz"));
		assert!(!matches("ab", "ba"));
	}

	#[test]
	fn names() {
		let heading = Export::new("intro", SymbolKind::Heading, None).with("title", "Intro");
		assert_eq!(symbol_name("guide", &heading), "Intro (guide#intro)");
		let anchor = Export::new("here", SymbolKind::Anchor, None);
		assert_eq!(symbol_name("guide", &anchor), "guide#here");
		let variable = Export::new("version", SymbolKind::Variable, None).with("value", "1.2");
		assert_eq!(symbol_name("config", &variable), "config.version = 1.2");
	}
}
