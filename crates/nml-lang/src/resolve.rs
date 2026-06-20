use std::collections::HashMap;
use std::sync::Arc;

use nml_source::Source;
use nml_source::Url;

/// Why a file or module cannot be found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
	/// No file at this workspace path (or the path leaves the workspace).
	NotFound { path: String },
	/// No `[[module]]` with this name.
	UnknownModule { name: String },
}

/// Finds the files a unit includes and the modules it imports. Paths are relative to the
/// workspace root, with `/` separators.
pub trait Resolver: Sync {
	/// The file `path` included from the file at `from`: its workspace path and source.
	fn include(&self, from: &str, path: &str) -> Result<(String, Arc<Source>), ResolveError>;

	/// The workspace path of a module (`std.toc`).
	fn module(&self, module: &str) -> Result<String, ResolveError>;
}

/// The workspace path of `path` written in the file at `from`: relative to that file's
/// directory, or to the workspace root with a leading `/`, with `.nml` added if missing.
/// `None` if it leaves the workspace.
pub fn resolve_path(from: &str, path: &str) -> Option<String> {
	let mut parts: Vec<&str> = match path.strip_prefix('/') {
		Some(_) => Vec::new(),
		None => {
			let mut dir: Vec<&str> = from.split('/').collect();
			dir.pop();
			dir
		}
	};
	for part in path.split('/') {
		match part {
			"" | "." => {}
			".." => {
				parts.pop()?;
			}
			part => parts.push(part),
		}
	}
	let mut resolved = parts.join("/");
	if resolved.is_empty() {
		return None;
	}
	if !resolved.ends_with(".nml") {
		resolved.push_str(".nml");
	}
	Some(resolved)
}

/// Files and modules held in memory, for tests and unsaved buffers.
#[derive(Debug, Default, Clone)]
pub struct MemoryResolver {
	pub files: HashMap<String, String>,
	/// Module name to root directory.
	pub modules: HashMap<String, String>,
}

impl Resolver for MemoryResolver {
	fn include(&self, from: &str, path: &str) -> Result<(String, Arc<Source>), ResolveError> {
		let not_found = || ResolveError::NotFound { path: path.into() };
		let resolved = resolve_path(from, path).ok_or_else(not_found)?;
		let content = self.files.get(&resolved).ok_or_else(not_found)?;
		let url = Url::parse(&format!("file:///{resolved}")).unwrap();
		let source = Arc::new(Source::new(url, content.clone(), 0));
		Ok((resolved, source))
	}

	fn module(&self, module: &str) -> Result<String, ResolveError> {
		let (name, rest) = module.split_once('.').unwrap_or((module, ""));
		let root = self
			.modules
			.get(name)
			.ok_or_else(|| ResolveError::UnknownModule { name: name.into() })?;
		let path = format!("{root}/{}.nml", rest.replace('.', "/"));
		if rest.is_empty() || !self.files.contains_key(&path) {
			return Err(ResolveError::NotFound { path });
		}
		Ok(path)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn paths() {
		for (from, path, expected) in [
			("a.nml", "b", Some("b.nml")),
			("a.nml", "b.nml", Some("b.nml")),
			("docs/a.nml", "parts/b", Some("docs/parts/b.nml")),
			("docs/a.nml", "../b", Some("b.nml")),
			("docs/a.nml", "./b", Some("docs/b.nml")),
			("docs/a.nml", "/parts/b", Some("parts/b.nml")),
			("a.nml", "../b", None),
			("a.nml", "/", None),
		] {
			assert_eq!(
				resolve_path(from, path).as_deref(),
				expected,
				"{path} from {from}"
			);
		}
	}
}
