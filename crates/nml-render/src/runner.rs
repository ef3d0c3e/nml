//! Runners: programs that run the code of `:run` blocks, declared by the workspace.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// The default time limit of a runner, in seconds.
pub const DEFAULT_RUN_TIMEOUT: u64 = 300;

/// The placeholder of a runner's command that stands for the file of the code.
pub const FILE_PLACEHOLDER: &str = "{file}";

/// The environment variable of the directory where a run writes its output files.
pub const OUT_VARIABLE: &str = "NML_OUT";

/// The environment variable of the directory holding the data a run is given: tables and
/// variables, as files.
pub const IN_VARIABLE: &str = "NML_IN";

/// The environment variable of the workspace root, for a run.
pub const ROOT_VARIABLE: &str = "NML_ROOT";

/// How many bytes of the end of a failed run's standard error make its message.
const MESSAGE_LENGTH: usize = 2000;

/// A runner of the configuration: `[run.<name>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runner {
	/// The program and its arguments. `{file}` is replaced by the file of the code; without
	/// it, the code is the program's standard input.
	pub command: Vec<String>,
	/// The extension of the code's file, without the dot.
	pub extension: String,
	/// The time limit, in seconds.
	pub timeout: u64,
	/// Variables added to the environment of the program.
	pub env: BTreeMap<String, String>,
}

impl Runner {
	/// What a run's output depends on besides its code and inputs: part of its cache key.
	pub fn key(&self) -> String {
		let mut key = self.command.join("\0");
		key.push_str("\0\0");
		key.push_str(&self.extension);
		for (name, value) in &self.env {
			key.push_str(&format!("\0{name}={value}"));
		}
		key
	}

	/// Runs `code`, in the directory `cwd`, with the workspace at `root`. `data` are the
	/// files of `$NML_IN`, by name. The program writes its files into `out`, an empty
	/// directory, which is created if needed.
	pub fn run(
		&self,
		code: &str,
		data: &[(String, String)],
		cwd: &Path,
		root: &Path,
		out: &Path,
	) -> RunResult {
		let failed = |message: String, duration: Duration| RunResult {
			status: RunStatus::Failed,
			stdout: String::new(),
			stderr: String::new(),
			message: Some(message),
			duration,
			files: Vec::new(),
		};
		let Some(program) = self.command.first() else {
			return failed("the runner has no command".into(), Duration::ZERO);
		};
		if let Err(error) = std::fs::create_dir_all(out) {
			return failed(
				format!("cannot create {}: {error}", out.display()),
				Duration::ZERO,
			);
		}
		let scratch = match tempdir() {
			Ok(dir) => dir,
			Err(error) => return failed(error, Duration::ZERO),
		};
		let data_dir = scratch.join("in");
		let written = std::fs::create_dir_all(&data_dir).and_then(|()| {
			(data.iter())
				.try_for_each(|(name, content)| std::fs::write(data_dir.join(name), content))
		});
		if let Err(error) = written {
			let _ = std::fs::remove_dir_all(&scratch);
			return failed(
				format!("cannot write the run's data: {error}"),
				Duration::ZERO,
			);
		}
		let file = scratch.join(format!("code.{}", self.extension));
		let uses_file = self
			.command
			.iter()
			.any(|arg| arg.contains(FILE_PLACEHOLDER));
		if uses_file && let Err(error) = std::fs::write(&file, code) {
			let _ = std::fs::remove_dir_all(&scratch);
			return failed(
				format!("cannot write {}: {error}", file.display()),
				Duration::ZERO,
			);
		}
		let file_arg = file.to_string_lossy();
		let mut command = Command::new(program);
		command
			.args(
				self.command[1..]
					.iter()
					.map(|arg| arg.replace(FILE_PLACEHOLDER, &file_arg)),
			)
			.current_dir(cwd)
			.envs(&self.env)
			.env(OUT_VARIABLE, absolute(out))
			.env(IN_VARIABLE, &data_dir)
			.env(ROOT_VARIABLE, absolute(root));
		let input = if uses_file { "" } else { code };
		let finished = crate::process::wait(
			&mut command,
			input.as_bytes(),
			Duration::from_secs(self.timeout),
		);
		let _ = std::fs::remove_dir_all(&scratch);
		let finished = match finished {
			Ok(finished) => finished,
			Err(error) => {
				return failed(format!("cannot run `{program}`: {error}"), Duration::ZERO);
			}
		};
		let stdout = String::from_utf8_lossy(&finished.out).into_owned();
		let stderr = String::from_utf8_lossy(&finished.err).into_owned();
		let (status, message) = match finished.status {
			None => (
				RunStatus::Failed,
				Some(format!("`{program}` took longer than {} s", self.timeout)),
			),
			Some(status) if status.success() => (RunStatus::Completed, None),
			Some(status) => {
				let tail = tail(stderr.trim_end(), MESSAGE_LENGTH);
				let message = match status.code() {
					Some(code) if tail.is_empty() => format!("`{program}` exited with {code}"),
					None if tail.is_empty() => format!("`{program}` was stopped by a signal"),
					_ => tail.to_string(),
				};
				(RunStatus::Failed, Some(message))
			}
		};
		RunResult {
			status,
			stdout,
			stderr,
			message,
			duration: finished.duration,
			files: match status {
				RunStatus::Completed => files(out),
				RunStatus::Failed => Vec::new(),
			},
		}
	}
}

/// What the earlier blocks of a session give a block: their code and data, which run
/// before its own, and what they printed and wrote, which is left out of its result.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SessionPrefix {
	/// Their code, in order, each followed by a line break.
	pub code: String,
	/// Their data, by file name; a later block's file replaces an earlier one's.
	pub data: Vec<(String, String)>,
	/// What they printed, in order.
	pub stdout: String,
	/// The files they wrote, by name, with the hash of their content.
	pub files: Vec<(String, String)>,
	/// The key of the block before.
	pub key: String,
	/// Whether every earlier block completed: otherwise the block does not run.
	pub complete: bool,
}

/// Whether a run completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunStatus {
	/// The program exited successfully.
	Completed,
	/// It could not start, exited with an error, or ran out of time.
	Failed,
}

impl RunStatus {
	pub fn name(self) -> &'static str {
		match self {
			RunStatus::Completed => "completed",
			RunStatus::Failed => "failed",
		}
	}

	pub fn from_name(name: &str) -> Option<Self> {
		match name {
			"completed" => Some(RunStatus::Completed),
			"failed" => Some(RunStatus::Failed),
			_ => None,
		}
	}
}

/// What a run did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
	pub status: RunStatus,
	pub stdout: String,
	pub stderr: String,
	/// Why it failed.
	pub message: Option<String>,
	pub duration: Duration,
	/// The files it wrote, relative to its output directory with `/` separators, sorted.
	pub files: Vec<String>,
}

/// The end of `text`, at most `length` bytes, from the start of a line.
fn tail(text: &str, length: usize) -> &str {
	if text.len() <= length {
		return text;
	}
	let mut start = text.len() - length;
	while !text.is_char_boundary(start) {
		start += 1;
	}
	let rest = &text[start..];
	if text.as_bytes()[start - 1] == b'\n' {
		return rest;
	}
	match rest.find('\n') {
		Some(line) => &rest[line + 1..],
		None => rest,
	}
}

fn absolute(path: &Path) -> std::path::PathBuf {
	std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The regular files under `dir`, at any depth, relative to it.
fn files(dir: &Path) -> Vec<String> {
	fn walk(dir: &Path, prefix: &str, files: &mut Vec<String>) {
		let Ok(entries) = std::fs::read_dir(dir) else {
			return;
		};
		for entry in entries.flatten() {
			let Ok(kind) = entry.file_type() else {
				continue;
			};
			let name = entry.file_name().to_string_lossy().into_owned();
			let path = format!("{prefix}{name}");
			if kind.is_dir() {
				walk(&entry.path(), &format!("{path}/"), files);
			} else if kind.is_file() {
				files.push(path);
			}
		}
	}
	let mut found = Vec::new();
	walk(dir, "", &mut found);
	found.sort();
	found
}

/// A new, empty temporary directory.
fn tempdir() -> Result<std::path::PathBuf, String> {
	use std::sync::atomic::AtomicU64;
	use std::sync::atomic::Ordering;
	static COUNT: AtomicU64 = AtomicU64::new(0);
	let name = format!(
		"nml-run-{}-{}",
		std::process::id(),
		COUNT.fetch_add(1, Ordering::Relaxed)
	);
	let path = std::env::temp_dir().join(name);
	std::fs::create_dir_all(&path)
		.map_err(|error| format!("cannot create {}: {error}", path.display()))?;
	Ok(path)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn sh(script: &str) -> Runner {
		Runner {
			command: vec!["sh".into(), "-c".into(), script.into()],
			extension: "sh".into(),
			timeout: 10,
			env: BTreeMap::new(),
		}
	}

	struct Dirs {
		base: std::path::PathBuf,
	}

	impl Dirs {
		fn new() -> Self {
			Self {
				base: tempdir().unwrap(),
			}
		}

		fn out(&self) -> std::path::PathBuf {
			self.base.join("out")
		}
	}

	impl Drop for Dirs {
		fn drop(&mut self) {
			let _ = std::fs::remove_dir_all(&self.base);
		}
	}

	#[test]
	fn code_on_stdin_and_output_files() {
		let dirs = Dirs::new();
		let runner = Runner {
			command: vec!["sh".into()],
			extension: "sh".into(),
			timeout: 10,
			env: BTreeMap::from([("GREETING".into(), "hello".into())]),
		};
		let code = "echo \"$GREETING\"\necho warn >&2\nmkdir \"$NML_OUT/sub\"\n\
			printf a > \"$NML_OUT/b.txt\"\nprintf c > \"$NML_OUT/sub/c.svg\"\n\
			ln -s b.txt \"$NML_OUT/link\"\npwd > \"$NML_OUT/cwd\"\n\
			printf %s \"$NML_ROOT\" > \"$NML_OUT/root\"";
		let result = runner.run(code, &[], &dirs.base, Path::new("/w"), &dirs.out());
		assert_eq!(result.status, RunStatus::Completed, "{result:?}");
		assert_eq!(result.stdout, "hello\n");
		assert_eq!(result.stderr, "warn\n");
		assert_eq!(result.message, None);
		// Symbolic links are not outputs.
		assert_eq!(result.files, ["b.txt", "cwd", "root", "sub/c.svg"]);
		let read = |name| std::fs::read_to_string(dirs.out().join(name)).unwrap();
		assert_eq!(read("sub/c.svg"), "c");
		assert_eq!(
			read("cwd").trim(),
			dirs.base.canonicalize().unwrap().to_string_lossy()
		);
		assert_eq!(read("root"), "/w");
	}

	#[test]
	fn data_in_files() {
		let dirs = Dirs::new();
		let data = [
			("sales.csv".to_string(), "a,1\n".to_string()),
			("version.txt".to_string(), "1.2".to_string()),
		];
		let result = sh("cat \"$NML_IN/sales.csv\" \"$NML_IN/version.txt\"").run(
			"",
			&data,
			&dirs.base,
			&dirs.base,
			&dirs.out(),
		);
		assert_eq!(result.stdout, "a,1\n1.2", "{result:?}");
		// Without data, the directory is empty.
		let result =
			sh("ls -A \"$NML_IN\" | wc -l").run("", &[], &dirs.base, &dirs.base, &dirs.out());
		assert_eq!(result.stdout.trim(), "0");
	}

	#[test]
	fn code_in_a_file() {
		let dirs = Dirs::new();
		let runner = Runner {
			command: vec![
				"sh".into(),
				"-c".into(),
				"cat \"$0\"; echo \"$0\"".into(),
				"{file}".into(),
			],
			extension: "R".into(),
			timeout: 10,
			env: BTreeMap::new(),
		};
		let result = runner.run("plot(x)\n", &[], &dirs.base, &dirs.base, &dirs.out());
		assert_eq!(result.status, RunStatus::Completed, "{result:?}");
		let mut lines = result.stdout.lines();
		assert_eq!(lines.next(), Some("plot(x)"));
		let file = lines.next().unwrap();
		assert!(file.ends_with("/code.R"), "{file}");
		// The file is removed afterwards.
		assert!(!Path::new(file).exists());
	}

	#[test]
	fn failures() {
		let dirs = Dirs::new();
		let result = sh("echo out; echo first >&2; echo second >&2; exit 3").run(
			"",
			&[],
			&dirs.base,
			&dirs.base,
			&dirs.out(),
		);
		assert_eq!(result.status, RunStatus::Failed);
		assert_eq!(result.stdout, "out\n");
		assert_eq!(result.message.as_deref(), Some("first\nsecond"));
		assert!(result.files.is_empty());

		let result = sh("exit 2").run("", &[], &dirs.base, &dirs.base, &dirs.out());
		assert_eq!(result.message.as_deref(), Some("`sh` exited with 2"));

		let mut slow = sh("sleep 5");
		slow.timeout = 0;
		let result = slow.run("", &[], &dirs.base, &dirs.base, &dirs.out());
		assert_eq!(result.status, RunStatus::Failed);
		assert_eq!(result.message.as_deref(), Some("`sh` took longer than 0 s"));
		assert!(result.duration < Duration::from_secs(4));

		let missing = Runner {
			command: vec!["nml-no-such-program".into()],
			..sh("")
		};
		let result = missing.run("", &[], &dirs.base, &dirs.base, &dirs.out());
		assert_eq!(result.status, RunStatus::Failed);
		assert!(
			(result.message.as_deref().unwrap()).starts_with("cannot run `nml-no-such-program`"),
			"{result:?}"
		);
	}

	#[test]
	fn long_errors_keep_their_end() {
		let text = "a\n".repeat(2000) + "last";
		assert_eq!(tail(&text, 10), "a\na\na\nlast");
		assert_eq!(tail(&text, 9), "a\na\nlast");
		assert_eq!(tail("short", 10), "short");
	}

	#[test]
	fn keys() {
		let a = sh("x");
		let mut b = a.clone();
		assert_eq!(a.key(), b.key());
		b.env.insert("A".into(), "1".into());
		assert_ne!(a.key(), b.key());
		let mut c = a.clone();
		c.extension = "py".into();
		assert_ne!(a.key(), c.key());
		// The timeout does not change what a run makes.
		let mut d = a.clone();
		d.timeout = 1;
		assert_eq!(a.key(), d.key());
	}
}
