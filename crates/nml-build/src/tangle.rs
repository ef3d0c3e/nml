//! `nml tangle`: the code blocks with `tangle=path`, written to their files.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_lang::Resolver;
use nml_lang::Tangle;
use nml_workspace::Index;

use crate::BuildError;
use crate::WorkspaceResolver;

/// What tangling did.
#[derive(Debug, Default)]
pub struct TangleReport {
	/// The files written, by workspace path.
	pub written: Vec<String>,
	/// The files that already had their code.
	pub unchanged: Vec<String>,
	pub diagnostics: Vec<Diagnostic>,
}

impl TangleReport {
	pub fn has_errors(&self) -> bool {
		crate::has_errors(&self.diagnostics)
	}
}

/// Writes the code blocks of the workspace's units to the files they are tangled to, from
/// the files as they are on disk. A file is tangled from one unit, its included files
/// included, in the unit's order.
pub fn tangle(root: &Path) -> Result<TangleReport, BuildError> {
	let root = fs::canonicalize(root).map_err(|e| BuildError::Io(root.into(), e))?;
	let config = crate::config(&root)?;
	let index = Index::open(&root.join(&config.database))?;
	let scan = nml_workspace::scan(&root, &config, &index)?;
	let resolver: Arc<dyn Resolver> = Arc::new(
		WorkspaceResolver::new(root.clone(), config.modules.clone()).with_runs(
			config.runners.clone(),
			crate::run_cache(&root, &config),
			false,
		),
	);
	let language = crate::language();
	let mut report = TangleReport::default();
	// By file to write, by unit: the blocks, in the unit's order.
	let mut files: BTreeMap<String, BTreeMap<String, Vec<Tangle>>> = BTreeMap::new();
	for file in &scan.files {
		let source = match &file.content {
			Some(content) => crate::source(&file.absolute, content.clone()),
			None => crate::read(&file.absolute)?,
		};
		let compiled = nml_lang::compile(&language, source, &file.path, resolver.clone());
		// Only the errors of the blocks themselves: the build reports the others.
		report
			.diagnostics
			.extend(compiled.diagnostics.into_iter().filter(|diagnostic| {
				matches!(diagnostic.kind, DiagnosticKind::InvalidTanglePath { .. })
			}));
		for tangle in compiled.tangles {
			let units = files.entry(tangle.path.clone()).or_default();
			units.entry(file.path.clone()).or_default().push(tangle);
		}
	}
	// An included file's errors are found in each unit that includes it.
	let mut seen = std::collections::HashSet::new();
	report.diagnostics.retain(|diagnostic| {
		let location = &diagnostic.primary().location;
		seen.insert((
			location.root().source().location().clone(),
			location.root().range(),
		))
	});
	let place = |tangle: &Tangle| (tangle.file.clone(), tangle.token.root().start());
	for (path, units) in files {
		// The blocks, wherever they are found, and the unit that has them all: an included
		// file's blocks are in its includer too.
		let all: std::collections::BTreeSet<_> = units.values().flatten().map(place).collect();
		let whole = units.values().find(|blocks| {
			blocks
				.iter()
				.map(place)
				.collect::<std::collections::BTreeSet<_>>()
				== all
		});
		let Some(blocks) = whole else {
			// The largest unit's file is the first; the blocks of the others conflict.
			let (unit, largest) = units.iter().max_by_key(|(_, blocks)| blocks.len()).unwrap();
			let theirs: std::collections::BTreeSet<_> = largest.iter().map(place).collect();
			let mut reported = std::collections::BTreeSet::new();
			for tangle in units.values().flatten() {
				if theirs.contains(&place(tangle)) || !reported.insert(place(tangle)) {
					continue;
				}
				let kind = DiagnosticKind::TangleConflict {
					path: path.clone(),
					other: unit.clone(),
				};
				let message = DiagnosticMessage::new(tangle.token.clone());
				report.diagnostics.push(Diagnostic::new(kind, message));
			}
			continue;
		};
		let code: Vec<&str> = blocks.iter().map(|tangle| tangle.code.as_str()).collect();
		let content = format!("{}\n", code.join("\n\n"));
		let target = root.join(&path);
		if fs::read_to_string(&target).is_ok_and(|current| current == content) {
			report.unchanged.push(path);
			continue;
		}
		if let Some(parent) = target.parent() {
			fs::create_dir_all(parent).map_err(|e| BuildError::Io(parent.into(), e))?;
		}
		fs::write(&target, content).map_err(|e| BuildError::Io(target.clone(), e))?;
		report.written.push(path);
	}
	Ok(report)
}
