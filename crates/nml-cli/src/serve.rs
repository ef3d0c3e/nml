//! `nml serve`: the live preview without an editor. The workspace's files are watched, and
//! each change is built (without writing) and shown in the browsers.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use std::time::SystemTime;

use nml_build::BuildOptions;
use nml_preview::PreviewServer;

/// How often the files are looked at.
const POLL: Duration = Duration::from_millis(300);

/// The files that make the workspace, with when they changed and their size.
type Snapshot = BTreeMap<PathBuf, (Option<SystemTime>, u64)>;

pub(crate) fn serve(root: &Path, port: u16, open: bool) -> ExitCode {
	let Some(watched) = build(root) else {
		return ExitCode::FAILURE;
	};
	let server = match PreviewServer::start_on(root, port) {
		Ok(server) => server,
		Err(error) => {
			eprintln!("error: {error}");
			return ExitCode::FAILURE;
		}
	};
	let url = format!("http://{}/", server.address());
	eprintln!("serving {} at {url}", root.display());
	if open {
		open_browser(&url);
	}

	let mut watched = watched;
	let mut last = snapshot(&watched);
	loop {
		std::thread::sleep(POLL);
		let now = snapshot(&watched);
		if now == last {
			continue;
		}
		// Editors write in steps: the change is built once it settles.
		let mut settled = now;
		loop {
			std::thread::sleep(POLL);
			let again = snapshot(&watched);
			if again == settled {
				break;
			}
			settled = again;
		}
		if let Some(again) = build(root) {
			watched = again;
			server.saved();
		}
		last = snapshot(&watched);
	}
}

/// What is watched: the workspace and the roots of its modules, except its output, its
/// index, and hidden files (`.git`, `.nml`).
struct Watched {
	roots: Vec<PathBuf>,
	skip: Vec<PathBuf>,
}

/// Builds the workspace without writing, and reports its diagnostics. `None` if it cannot be
/// built, such as when its configuration is invalid: the error is reported.
fn build(root: &Path) -> Option<Watched> {
	match nml_build::build_with(root, BuildOptions { write: false }) {
		Ok(report) => {
			crate::print_diagnostics(&report.diagnostics);
			let errors = report.has_errors();
			eprintln!(
				"{} parsed{}",
				report.parsed.len(),
				if errors { ", with errors" } else { "" }
			);
		}
		Err(error) => eprintln!("error: {error}"),
	}
	// What to watch, even when the build failed: its fix is a change too.
	let root = fs::canonicalize(root).ok()?;
	let config = nml_build::config(&root).ok();
	let mut roots = vec![root.clone()];
	let mut skip = Vec::new();
	if let Some(config) = config {
		skip.push(root.join(&config.output));
		skip.push(root.join(&config.database));
		for module in &config.modules {
			let path = root.join(&module.root);
			let path = fs::canonicalize(&path).unwrap_or(path);
			if !path.starts_with(&root) {
				roots.push(path);
			}
		}
	}
	Some(Watched { roots, skip })
}

fn snapshot(watched: &Watched) -> Snapshot {
	let mut files = Snapshot::new();
	for root in &watched.roots {
		walk(root, &watched.skip, &mut files);
	}
	files
}

fn walk(dir: &Path, skip: &[PathBuf], files: &mut Snapshot) {
	let Ok(entries) = fs::read_dir(dir) else {
		return;
	};
	for entry in entries.flatten() {
		let path = entry.path();
		let hidden = entry.file_name().to_string_lossy().starts_with('.');
		// The index's journal files (`index.db-wal`) are skipped with it.
		let skipped = skip.iter().any(|skip| {
			path == *skip
				|| path
					.to_str()
					.zip(skip.to_str())
					.is_some_and(|(path, skip)| path.starts_with(&format!("{skip}-")))
		});
		if hidden || skipped {
			continue;
		}
		let Ok(file_type) = entry.file_type() else {
			continue;
		};
		if file_type.is_dir() {
			walk(&path, skip, files);
		} else if let Ok(metadata) = entry.metadata() {
			files.insert(path, (metadata.modified().ok(), metadata.len()));
		}
	}
}

/// Opens `url` in the system's browser; it is printed anyway.
fn open_browser(url: &str) {
	let command = if cfg!(target_os = "macos") {
		("open", vec![url])
	} else if cfg!(windows) {
		("cmd", vec!["/C", "start", "", url])
	} else {
		("xdg-open", vec![url])
	};
	let spawned = std::process::Command::new(command.0)
		.args(command.1)
		.stdout(std::process::Stdio::null())
		.stderr(std::process::Stdio::null())
		.spawn();
	if let Err(error) = spawned {
		eprintln!("cannot open a browser ({error})");
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn watches_the_workspace_and_its_modules() {
		let dir = std::env::temp_dir().join(format!("nml-serve-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		let files = [
			(
				"ws/nml.toml",
				"output = \"site\"\ndatabase = \"index.db\"\n\n[[module]]\nname = \"lib\"\nroot = \"../lib\"\n",
			),
			("ws/a.nml", "# A\n"),
			("ws/img/b.png", "png"),
			("ws/site/a.html", "out"),
			("ws/index.db", "db"),
			("ws/index.db-wal", "db"),
			("ws/.git/HEAD", "ref"),
			("ws/.nml/std/toc.nml", "toc"),
			("lib/util.nml", "util"),
		];
		for (path, content) in files {
			let path = dir.join(path);
			fs::create_dir_all(path.parent().unwrap()).unwrap();
			fs::write(path, content).unwrap();
		}
		let root = dir.join("ws");
		let watched = build(&root).unwrap();
		let dir = fs::canonicalize(&dir).unwrap();
		let names: Vec<_> = snapshot(&watched)
			.into_keys()
			.map(|path| {
				path.strip_prefix(&dir)
					.unwrap()
					.to_string_lossy()
					.into_owned()
			})
			.collect();
		assert_eq!(
			names,
			["lib/util.nml", "ws/a.nml", "ws/img/b.png", "ws/nml.toml"]
		);

		// A change shows in the snapshot.
		let before = snapshot(&watched);
		fs::write(dir.join("ws/a.nml"), "# A, longer\n").unwrap();
		assert_ne!(snapshot(&watched), before);

		// An invalid configuration: the workspace is still watched, to see it fixed.
		fs::write(dir.join("ws/nml.toml"), "pages = 1\n").unwrap();
		let watched = build(&root).unwrap();
		assert_eq!(watched.roots, [dir.join("ws")]);
		fs::remove_dir_all(&dir).unwrap();
	}
}
