use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use crate::Config;
use crate::FileState;
use crate::Index;
use crate::IndexError;
use crate::Role;

/// How a file compares with its committed unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
	/// No unit: a new file, or one whose last parse failed.
	Added,
	Modified,
	/// Same content. The state may still differ (a touched file); see
	/// [`ScannedFile::refresh`].
	Unchanged,
}

/// A `.nml` file of the workspace.
#[derive(Debug, Clone)]
pub struct ScannedFile {
	/// Relative to the workspace root, with `/` separators.
	pub path: String,
	pub absolute: PathBuf,
	pub role: Role,
	pub state: FileState,
	/// The content, if the scan had to read it.
	pub content: Option<String>,
	pub change: Change,
}

impl ScannedFile {
	/// Whether the stored state should be updated: unchanged content with a new mtime.
	pub fn refresh(&self, stored: &FileState) -> bool {
		self.change == Change::Unchanged && self.state != *stored
	}
}

/// The result of a scan: every file, and the units whose file is gone.
#[derive(Debug, Default)]
pub struct Scan {
	/// By path.
	pub files: Vec<ScannedFile>,
	/// Paths of units without a file.
	pub removed: Vec<String>,
}

#[derive(Debug)]
pub enum ScanError {
	Io(PathBuf, io::Error),
	Index(IndexError),
}

impl std::fmt::Display for ScanError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ScanError::Io(path, error) => write!(f, "{}: {error}", path.display()),
			ScanError::Index(error) => error.fmt(f),
		}
	}
}

impl std::error::Error for ScanError {}

impl From<IndexError> for ScanError {
	fn from(error: IndexError) -> Self {
		Self::Index(error)
	}
}

/// Files modified this recently are hashed by the next scan as well, since timestamps can
/// be too coarse to see an edit made right after the scan, in nanoseconds.
const RACY_WINDOW: i64 = 2_000_000_000;

/// Hashes file content.
pub fn hash(content: &[u8]) -> Vec<u8> {
	blake3::hash(content).as_bytes().to_vec()
}

/// Compares the workspace's files with the index. A file whose `(mtime, size)` match its
/// unit is unchanged without being read; otherwise its content hash decides.
///
/// The output directory, the static directory, `.nml/` and hidden directories are skipped.
pub fn scan(root: &Path, config: &Config, index: &Index) -> Result<Scan, ScanError> {
	let mut paths = Vec::new();
	let mut skip = vec![root.join(&config.output), root.join(".nml")];
	skip.extend(config.html.static_dir.as_ref().map(|dir| root.join(dir)));
	collect(root, &skip, &mut paths)?;
	paths.sort();

	let now = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map_or(0, |d| d.as_nanos() as i64);
	let mut units = index.units()?;
	let mut files = Vec::new();
	for absolute in paths {
		let relative = absolute.strip_prefix(root).unwrap();
		let path = relative
			.components()
			.map(|c| c.as_os_str().to_string_lossy())
			.collect::<Vec<_>>()
			.join("/");
		let role = if config.is_page(relative) {
			Role::Page
		} else {
			Role::Library
		};
		let io_error = |e| ScanError::Io(absolute.clone(), e);
		let metadata = fs::metadata(&absolute).map_err(io_error)?;
		let mtime = metadata
			.modified()
			.map_err(io_error)?
			.duration_since(UNIX_EPOCH)
			.map_or(0, |d| d.as_nanos() as i64);
		let size = metadata.len();
		// A file modified within the racy window could change again without its mtime
		// changing. Its state is stored with no mtime, so the next scan hashes it.
		let stored_mtime = if now - mtime < RACY_WINDOW { 0 } else { mtime };

		let stored = units
			.iter()
			.position(|unit| unit.path == path)
			.map(|i| units.swap_remove(i));
		let fast = stored.as_ref().is_some_and(|unit| {
			unit.role == role && unit.state.mtime == mtime && unit.state.size == size
		});
		let (state, content, change) = if fast {
			let state = stored.unwrap().state;
			(state, None, Change::Unchanged)
		} else {
			let bytes = fs::read(&absolute).map_err(io_error)?;
			let state = FileState {
				mtime: stored_mtime,
				size,
				hash: hash(&bytes),
			};
			let content = String::from_utf8(bytes).map_err(|e| {
				ScanError::Io(
					absolute.clone(),
					io::Error::new(io::ErrorKind::InvalidData, e),
				)
			})?;
			let change = match stored {
				None => Change::Added,
				Some(unit) if unit.role == role && unit.state.hash == state.hash => {
					Change::Unchanged
				}
				Some(_) => Change::Modified,
			};
			(state, Some(content), change)
		};
		files.push(ScannedFile {
			path,
			absolute,
			role,
			state,
			content,
			change,
		});
	}
	let removed = units.into_iter().map(|unit| unit.path).collect();
	Ok(Scan { files, removed })
}

/// The `.nml` files under `dir`, except in the `skip` directories and hidden ones.
fn collect(dir: &Path, skip: &[PathBuf], files: &mut Vec<PathBuf>) -> Result<(), ScanError> {
	let entries = fs::read_dir(dir).map_err(|e| ScanError::Io(dir.into(), e))?;
	for entry in entries {
		let entry = entry.map_err(|e| ScanError::Io(dir.into(), e))?;
		let path = entry.path();
		let hidden = entry.file_name().to_string_lossy().starts_with('.');
		let file_type = entry
			.file_type()
			.map_err(|e| ScanError::Io(path.clone(), e))?;
		if file_type.is_dir() && !hidden && !skip.contains(&path) {
			collect(&path, skip, files)?;
		} else if file_type.is_file() && path.extension().is_some_and(|e| e == "nml") {
			files.push(path);
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use nml_source::Source;
	use nml_source::Url;

	use super::*;
	use crate::StoredUnit;

	struct Workspace(PathBuf);

	impl Drop for Workspace {
		fn drop(&mut self) {
			let _ = fs::remove_dir_all(&self.0);
		}
	}

	impl Workspace {
		fn new(name: &str) -> Self {
			let dir = std::env::temp_dir().join(format!("nml-scan-{name}-{}", std::process::id()));
			let _ = fs::remove_dir_all(&dir);
			fs::create_dir_all(&dir).unwrap();
			Self(dir)
		}

		/// Writes a file, with a modification time `age` seconds ago, out of the racy
		/// window unless `age` is 0.
		fn write_aged(&self, path: &str, content: &str, age: u64) {
			let path = self.0.join(path);
			fs::create_dir_all(path.parent().unwrap()).unwrap();
			fs::write(&path, content).unwrap();
			let mtime = SystemTime::now() - std::time::Duration::from_secs(age);
			fs::File::options()
				.write(true)
				.open(&path)
				.unwrap()
				.set_modified(mtime)
				.unwrap();
		}

		fn write(&self, path: &str, content: &str) {
			self.write_aged(path, content, 60);
		}
	}

	fn config(text: &str) -> Config {
		let url = Url::parse("file:///w/nml.toml").unwrap();
		Config::parse(Arc::new(Source::new(url, text.into(), 0))).unwrap()
	}

	fn commit_all(index: &mut Index, scan: &Scan) {
		for file in &scan.files {
			index
				.commit(&StoredUnit {
					path: &file.path,
					name: "unit",
					title: "Unit",
					role: file.role,
					state: file.state.clone(),
					exports: &[],
					uses: &[],
					dependencies: &[],
					template: None,
					output: "a.html",
					output_range: None,
					assets: &[],
					tasks: &[],
				})
				.unwrap();
		}
	}

	fn changes(scan: &Scan) -> Vec<(&str, Change, bool)> {
		scan.files
			.iter()
			.map(|f| (f.path.as_str(), f.change, f.content.is_some()))
			.collect()
	}

	#[test]
	fn classifies_files() {
		let workspace = Workspace::new("classify");
		let root = &workspace.0;
		workspace.write("a.nml", "a");
		workspace.write("lib/b.nml", "b");
		workspace.write("public/out.nml", "output");
		workspace.write(".nml/x.nml", "database dir");
		workspace.write(".git/y.nml", "hidden");
		workspace.write("notes.txt", "not nml");
		workspace.write("static/z.nml", "copied as it is");
		let config = config("exclude = [\"lib/**\"]\n[html]\nstatic = \"static\"");
		let mut index = Index::open_in_memory().unwrap();

		let first = scan(root, &config, &index).unwrap();
		assert_eq!(
			changes(&first),
			[
				("a.nml", Change::Added, true),
				("lib/b.nml", Change::Added, true)
			]
		);
		assert_eq!(first.files[0].role, Role::Page);
		assert_eq!(first.files[1].role, Role::Library);
		assert_eq!(first.files[0].content.as_deref(), Some("a"));
		commit_all(&mut index, &first);

		// Nothing changed: the fast path does not read the files.
		let second = scan(root, &config, &index).unwrap();
		assert_eq!(
			changes(&second),
			[
				("a.nml", Change::Unchanged, false),
				("lib/b.nml", Change::Unchanged, false)
			]
		);

		// A new size, a removed file and a new one.
		workspace.write("a.nml", "aa");
		fs::remove_file(root.join("lib/b.nml")).unwrap();
		workspace.write("c.nml", "c");
		let third = scan(root, &config, &index).unwrap();
		assert_eq!(
			changes(&third),
			[
				("a.nml", Change::Modified, true),
				("c.nml", Change::Added, true)
			]
		);
		assert_eq!(third.removed, ["lib/b.nml"]);
	}

	#[test]
	fn touched_files_are_hashed_and_refreshed() {
		let workspace = Workspace::new("touch");
		let root = &workspace.0;
		workspace.write("a.nml", "same");
		let config = config("");
		let mut index = Index::open_in_memory().unwrap();
		let first = scan(root, &config, &index).unwrap();
		commit_all(&mut index, &first);

		// Same content and size, another mtime.
		let mut stored = index.unit("a.nml").unwrap().unwrap().state;
		stored.mtime -= 1_000_000_000;
		index.update_state("a.nml", &stored).unwrap();
		let second = scan(root, &config, &index).unwrap();
		assert_eq!(changes(&second), [("a.nml", Change::Unchanged, true)]);
		assert!(second.files[0].refresh(&stored));

		index.update_state("a.nml", &second.files[0].state).unwrap();
		let third = scan(root, &config, &index).unwrap();
		assert_eq!(changes(&third), [("a.nml", Change::Unchanged, false)]);
	}

	#[test]
	fn same_size_edits_are_found_by_hash() {
		let workspace = Workspace::new("hash");
		let root = &workspace.0;
		workspace.write("a.nml", "abc");
		let config = config("");
		let mut index = Index::open_in_memory().unwrap();
		let first = scan(root, &config, &index).unwrap();
		commit_all(&mut index, &first);

		workspace.write("a.nml", "xyz");
		let mut stored = index.unit("a.nml").unwrap().unwrap().state;
		stored.mtime -= 1;
		index.update_state("a.nml", &stored).unwrap();
		let scanned = scan(root, &config, &index).unwrap();
		assert_eq!(changes(&scanned), [("a.nml", Change::Modified, true)]);
	}

	#[test]
	fn role_changes_are_modifications() {
		let workspace = Workspace::new("role");
		let root = &workspace.0;
		workspace.write("a.nml", "a");
		let mut index = Index::open_in_memory().unwrap();
		let first = scan(root, &config(""), &index).unwrap();
		commit_all(&mut index, &first);

		let scanned = scan(root, &config("pages = []"), &index).unwrap();
		assert_eq!(changes(&scanned), [("a.nml", Change::Modified, true)]);
		assert_eq!(scanned.files[0].role, Role::Library);
	}

	#[test]
	fn recent_files_are_always_hashed() {
		let workspace = Workspace::new("racy");
		let root = &workspace.0;
		workspace.write_aged("a.nml", "abc", 0);
		let config = config("");
		let mut index = Index::open_in_memory().unwrap();
		let first = scan(root, &config, &index).unwrap();
		assert_eq!(first.files[0].state.mtime, 0);
		commit_all(&mut index, &first);

		// An edit of the same size in the same timestamp tick is still found.
		let mtime = fs::metadata(root.join("a.nml"))
			.unwrap()
			.modified()
			.unwrap();
		fs::write(root.join("a.nml"), "xyz").unwrap();
		fs::File::options()
			.write(true)
			.open(root.join("a.nml"))
			.unwrap()
			.set_modified(mtime)
			.unwrap();
		let second = scan(root, &config, &index).unwrap();
		assert_eq!(changes(&second), [("a.nml", Change::Modified, true)]);
	}
}
