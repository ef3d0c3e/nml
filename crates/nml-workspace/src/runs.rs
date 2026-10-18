//! The results of `:run` blocks, kept on disk by key: `<dir>/<key>/result.json` and the
//! files the run wrote under `<dir>/<key>/out/`. A result is written in a directory of its
//! own and renamed into place, so readers, in other threads or processes, see whole results.

use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use nml_render::RunResult;
use nml_render::RunStatus;
use serde::Deserialize;
use serde::Serialize;

/// The directory of the run results, beside the database.
pub fn runs_dir(database: &Path) -> PathBuf {
	database
		.parent()
		.map_or_else(|| PathBuf::from("runs"), |parent| parent.join("runs"))
}

/// The run results of a workspace.
#[derive(Debug, Clone)]
pub struct RunCache {
	dir: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Stored {
	status: String,
	stdout: String,
	stderr: String,
	message: Option<String>,
	duration_ms: u64,
	files: Vec<String>,
}

const RESULT_FILE: &str = "result.json";
const OUT_DIR: &str = "out";

impl RunCache {
	pub fn new(dir: PathBuf) -> Self {
		Self { dir }
	}

	/// The directory of the files of the run `key`.
	pub fn out_dir(&self, key: &str) -> PathBuf {
		self.dir.join(key).join(OUT_DIR)
	}

	/// The result of the run `key`, if it ran.
	pub fn get(&self, key: &str) -> Option<RunResult> {
		if !is_key(key) {
			return None;
		}
		let content = std::fs::read_to_string(self.dir.join(key).join(RESULT_FILE)).ok()?;
		let stored: Stored = serde_json::from_str(&content).ok()?;
		Some(RunResult {
			status: RunStatus::from_name(&stored.status)?,
			stdout: stored.stdout,
			stderr: stored.stderr,
			message: stored.message,
			duration: Duration::from_millis(stored.duration_ms),
			files: stored.files,
		})
	}

	/// Runs `run` with an empty directory for its files, and keeps its result as the run
	/// `key`, in place of an earlier one.
	pub fn run(&self, key: &str, run: impl FnOnce(&Path) -> RunResult) -> RunResult {
		use std::sync::atomic::AtomicU64;
		use std::sync::atomic::Ordering;
		static COUNT: AtomicU64 = AtomicU64::new(0);
		assert!(is_key(key), "a run key is a hexadecimal hash");
		let staging = self.dir.join(format!(
			".{key}-{}-{}",
			std::process::id(),
			COUNT.fetch_add(1, Ordering::Relaxed)
		));
		let _ = std::fs::remove_dir_all(&staging);
		let result = run(&staging.join(OUT_DIR));
		let stored = Stored {
			status: result.status.name().into(),
			stdout: result.stdout.clone(),
			stderr: result.stderr.clone(),
			message: result.message.clone(),
			duration_ms: result.duration.as_millis() as u64,
			files: result.files.clone(),
		};
		let json = serde_json::to_string(&stored).expect("a result is serializable");
		let stored = std::fs::create_dir_all(&staging)
			.and_then(|()| std::fs::write(staging.join(RESULT_FILE), json))
			.and_then(|()| {
				let target = self.dir.join(key);
				// Another result may be there: an older one, or one that just ran elsewhere.
				let _ = std::fs::remove_dir_all(&target);
				std::fs::rename(&staging, &target)
			});
		if stored.is_err() {
			let _ = std::fs::remove_dir_all(&staging);
		}
		result
	}

	/// Deletes the results that are not in `keep`, and unfinished ones. Returns how many
	/// results were deleted.
	pub fn prune(&self, keep: &BTreeSet<String>) -> usize {
		let Ok(entries) = std::fs::read_dir(&self.dir) else {
			return 0;
		};
		let mut pruned = 0;
		for entry in entries.flatten() {
			let name = entry.file_name().to_string_lossy().into_owned();
			if is_key(&name) && keep.contains(&name) {
				continue;
			}
			// Unfinished results of runs still going on are left to them.
			if name.starts_with('.') {
				continue;
			}
			if std::fs::remove_dir_all(entry.path()).is_ok() {
				pruned += 1;
			}
		}
		pruned
	}
}

/// Whether `key` can be a run's key: hexadecimal, so that it is a plain directory name.
fn is_key(key: &str) -> bool {
	!key.is_empty() && key.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
	use super::*;

	struct Dir(PathBuf);

	impl Dir {
		fn new(name: &str) -> Self {
			let path = std::env::temp_dir().join(format!("nml-runs-{name}-{}", std::process::id()));
			let _ = std::fs::remove_dir_all(&path);
			Self(path)
		}
	}

	impl Drop for Dir {
		fn drop(&mut self) {
			let _ = std::fs::remove_dir_all(&self.0);
		}
	}

	fn result(status: RunStatus, files: &[&str]) -> RunResult {
		RunResult {
			status,
			stdout: "out".into(),
			stderr: "err".into(),
			message: (status == RunStatus::Failed).then(|| "boom".into()),
			duration: Duration::from_millis(1234),
			files: files.iter().map(|file| file.to_string()).collect(),
		}
	}

	#[test]
	fn results_are_kept_by_key() {
		let dir = Dir::new("kept");
		let cache = RunCache::new(dir.0.clone());
		assert_eq!(cache.get("abc"), None);
		let ran = cache.run("abc", |out| {
			std::fs::create_dir_all(out.join("sub")).unwrap();
			std::fs::write(out.join("sub/plot.svg"), "<svg/>").unwrap();
			result(RunStatus::Completed, &["sub/plot.svg"])
		});
		assert_eq!(ran.files, ["sub/plot.svg"]);
		assert_eq!(cache.get("abc"), Some(ran));
		assert_eq!(
			std::fs::read_to_string(cache.out_dir("abc").join("sub/plot.svg")).unwrap(),
			"<svg/>"
		);

		// A new result replaces the old one, files and all.
		let failed = cache.run("abc", |_| result(RunStatus::Failed, &[]));
		assert_eq!(cache.get("abc"), Some(failed));
		assert!(!cache.out_dir("abc").join("sub/plot.svg").exists());

		// Keys are directory names.
		assert_eq!(cache.get("../abc"), None);
	}

	#[test]
	fn pruning() {
		let dir = Dir::new("prune");
		let cache = RunCache::new(dir.0.clone());
		for key in ["a1", "b2", "c3"] {
			cache.run(key, |_| result(RunStatus::Completed, &[]));
		}
		std::fs::create_dir_all(dir.0.join(".d4-1-1")).unwrap();
		std::fs::create_dir_all(dir.0.join("junk")).unwrap();
		let keep = BTreeSet::from(["b2".to_string()]);
		assert_eq!(cache.prune(&keep), 3);
		assert!(cache.get("a1").is_none());
		assert!(cache.get("b2").is_some());
		assert!(dir.0.join(".d4-1-1").exists());
		assert!(!dir.0.join("junk").exists());
		// Nothing to prune without the directory.
		assert_eq!(RunCache::new(dir.0.join("none")).prune(&keep), 0);
	}

	#[test]
	fn directory_beside_the_database() {
		assert_eq!(
			runs_dir(Path::new("/w/.nml/index.db")),
			Path::new("/w/.nml/runs")
		);
	}
}
