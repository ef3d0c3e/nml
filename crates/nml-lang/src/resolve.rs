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

	/// A file a page shows, such as an image (`path` written in the file at `from`): its
	/// workspace path and the hash of its content (BLAKE3, in hexadecimal).
	fn asset(&self, from: &str, path: &str) -> Result<(String, String), ResolveError>;

	/// The value of an environment variable of the build, if it is set.
	fn env(&self, name: &str) -> Option<String>;

	/// When the build started, in seconds since the Unix epoch: the same for every unit.
	fn time(&self) -> i64;
}

/// What a unit records of an environment variable it read: the BLAKE3 hash of its value, in
/// hexadecimal, or nothing if it is not set. The value itself is not stored.
pub fn env_hash(value: Option<&str>) -> String {
	value
		.map(|value| blake3::hash(value.as_bytes()).to_hex().to_string())
		.unwrap_or_default()
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

/// The directory of the output where the files pages show are copied.
pub const ASSETS_DIR: &str = "assets";

/// Where a workspace file a page shows is copied in the output: under [`ASSETS_DIR`],
/// named by `hash`, the hex hash of its content, with its extension. The name stays as
/// long as the content does, files with the same content share a copy, and a new version
/// gets a new name, which browsers cannot have cached.
pub fn asset_output_path(path: &str, hash: &str) -> String {
	let name = path.rsplit('/').next().unwrap_or(path);
	let extension = match name.rsplit_once('.') {
		Some((stem, extension)) if !stem.is_empty() => format!(".{extension}"),
		_ => String::new(),
	};
	format!("{ASSETS_DIR}/{}{extension}", &hash[..hash.len().min(16)])
}

/// Whether `path` can be where a page is written (`nml.output`): a relative path in the
/// output directory, ending with `.html`, and outside [`ASSETS_DIR`].
pub fn is_output_path(path: &str) -> bool {
	let parts: Vec<&str> = path.split('/').collect();
	path.ends_with(".html")
		&& path.len() > ".html".len()
		&& parts.first() != Some(&ASSETS_DIR)
		&& parts.iter().all(|part| {
			!matches!(*part, "" | "." | "..")
				&& !part
					.chars()
					.any(|c| c == '\\' || c == ':' || c.is_control())
		})
}

/// The URL of `to`, a path in the output directory, relative to `from`, another path in
/// the output directory.
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
	/// The environment variables of the build.
	pub env: HashMap<String, String>,
	/// When the build started.
	pub time: i64,
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

	fn asset(&self, from: &str, path: &str) -> Result<(String, String), ResolveError> {
		let (resolved, content) = self.read(from, path)?;
		Ok((
			resolved,
			blake3::hash(content.as_bytes()).to_hex().to_string(),
		))
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

	fn env(&self, name: &str) -> Option<String> {
		self.env.get(name).cloned()
	}

	fn time(&self) -> i64 {
		self.time
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
			("a.html", "b.html", "b.html"),
			("a.html", "a.html", "a.html"),
			("docs/a.html", "b.html", "../b.html"),
			("a.html", "docs/b.html", "docs/b.html"),
			("docs/x/a.html", "docs/y/b.html", "../y/b.html"),
			("docs/x/a.html", "docs/x/b.html", "b.html"),
			("x/y/a.html", "z.html", "../../z.html"),
			("docs/a.html", "assets/0123.png", "../assets/0123.png"),
		] {
			assert_eq!(relative_output_url(from, to), url, "from {from} to {to}");
		}
	}

	#[test]
	fn asset_output_paths() {
		let hash = "0123456789abcdef0123";
		for (path, output) in [
			("img/a.png", "assets/0123456789abcdef.png"),
			("a.tar.gz", "assets/0123456789abcdef.gz"),
			("img/README", "assets/0123456789abcdef"),
			("img/.hidden", "assets/0123456789abcdef"),
			("v.1/clip", "assets/0123456789abcdef"),
		] {
			assert_eq!(asset_output_path(path, hash), output, "{path}");
		}
	}

	#[test]
	fn output_paths() {
		for (path, valid) in [
			("a.html", true),
			("guide/install.html", true),
			("assets.html", true),
			("x/assets/a.html", true),
			(".html", false),
			("a", false),
			("a.htm", false),
			("/a.html", false),
			("a//b.html", false),
			("./a.html", false),
			("../a.html", false),
			("x/../a.html", false),
			("assets/a.html", false),
			("a\\b.html", false),
			("c:a.html", false),
		] {
			assert_eq!(is_output_path(path), valid, "{path}");
		}
	}
}
