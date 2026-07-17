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
pub trait Resolver: Send + Sync {
	/// The file `path` included from the file at `from`: its workspace path and source.
	fn include(&self, from: &str, path: &str) -> Result<(String, Arc<Source>), ResolveError>;

	/// A file read by a script (`path` written in the file at `from`): its workspace path
	/// and content. Unlike includes, `.nml` is not added.
	fn read(&self, from: &str, path: &str) -> Result<(String, String), ResolveError>;

	/// A module (`std.toc`): its workspace path and source.
	fn module(&self, module: &str) -> Result<(String, Arc<Source>), ResolveError>;
}

/// The workspace path of `path` written in the file at `from`: relative to that file's
/// directory, or to the workspace root with a leading `/`, with `.nml` added if missing.
/// `None` if it leaves the workspace.
pub fn resolve_path(from: &str, path: &str) -> Option<String> {
	let mut resolved = resolve_file(from, path)?;
	if !resolved.ends_with(".nml") {
		resolved.push_str(".nml");
	}
	Some(resolved)
}

/// The workspace path of the file `path` written in the file at `from`: relative to that
/// file's directory, or to the workspace root with a leading `/`. `None` if it leaves the
/// workspace.
pub fn resolve_file(from: &str, path: &str) -> Option<String> {
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
	let resolved = parts.join("/");
	(!resolved.is_empty()).then_some(resolved)
}

/// The output path of a unit: its path with `.html`.
pub fn output_path(path: &str) -> String {
	match path.strip_suffix(".nml") {
		Some(stem) => format!("{stem}.html"),
		None => format!("{path}.html"),
	}
}

/// The URL of the page of unit `to`, relative to the page of unit `from`.
pub fn relative_url(from: &str, to: &str) -> String {
	relative_output_url(from, &output_path(to))
}

/// The URL of `to`, a path in the output directory, relative to the page of unit `from`.
pub fn relative_output_url(from: &str, to: &str) -> String {
	let from: Vec<&str> = from.split('/').collect();
	let to: Vec<&str> = to.split('/').collect();
	let from_dirs = &from[..from.len() - 1];
	let common = from_dirs
		.iter()
		.zip(&to[..to.len() - 1])
		.take_while(|(a, b)| a == b)
		.count();
	let mut parts = vec![".."; from_dirs.len() - common];
	parts.extend(&to[common..]);
	parts.join("/")
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

	fn read(&self, from: &str, path: &str) -> Result<(String, String), ResolveError> {
		let not_found = || ResolveError::NotFound { path: path.into() };
		let resolved = resolve_file(from, path).ok_or_else(not_found)?;
		let content = self.files.get(&resolved).ok_or_else(not_found)?;
		Ok((resolved, content.clone()))
	}

	fn module(&self, module: &str) -> Result<(String, Arc<Source>), ResolveError> {
		let (name, rest) = module.split_once('.').unwrap_or((module, ""));
		let root = self
			.modules
			.get(name)
			.ok_or_else(|| ResolveError::UnknownModule { name: name.into() })?;
		let path = format!("{root}/{}.nml", rest.replace('.', "/"));
		match self.files.get(&path) {
			Some(content) if !rest.is_empty() => {
				let url = Url::parse(&format!("file:///{path}")).unwrap();
				let source = Arc::new(Source::new(url, content.clone(), 0));
				Ok((path, source))
			}
			_ => Err(ResolveError::NotFound { path }),
		}
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
	#[test]
	fn relative_urls() {
		for (from, to, url) in [
			("a.nml", "b.nml", "b.html"),
			("a.nml", "a.nml", "a.html"),
			("docs/a.nml", "b.nml", "../b.html"),
			("a.nml", "docs/b.nml", "docs/b.html"),
			("docs/x/a.nml", "docs/y/b.nml", "../y/b.html"),
			("docs/x/a.nml", "docs/x/b.nml", "b.html"),
			("x/y/a.nml", "z.nml", "../../z.html"),
		] {
			assert_eq!(relative_url(from, to), url, "from {from} to {to}");
		}
	}
}
