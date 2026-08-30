//! `nml build` in a child process, whose environment the tests choose.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// A temporary workspace, removed when dropped.
struct Workspace(PathBuf);

impl Drop for Workspace {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

impl Workspace {
	fn new(name: &str, files: &[(&str, &str)]) -> Self {
		let dir = std::env::temp_dir().join(format!("nml-cli-{name}-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		for (path, content) in files {
			let path = dir.join(path);
			fs::create_dir_all(path.parent().unwrap()).unwrap();
			fs::write(path, content).unwrap();
		}
		Self(dir)
	}

	/// Builds with these environment variables set, and the others of `unset` removed: the
	/// summary line, and the body of `page`.
	fn build(&self, set: &[(&str, &str)], unset: &[&str], page: &str) -> (String, String) {
		let mut command = Command::new(env!("CARGO_BIN_EXE_nml"));
		command.arg("build").arg(&self.0);
		for (name, value) in set {
			command.env(name, value);
		}
		for name in unset {
			command.env_remove(name);
		}
		let output = command.output().unwrap();
		let stderr = String::from_utf8(output.stderr).unwrap();
		assert!(output.status.success(), "{stderr}");
		let summary = stderr.lines().last().unwrap().to_string();
		let html = fs::read_to_string(self.0.join("public").join(page)).unwrap();
		let start = html.find("<body>\n").unwrap() + 7;
		let end = html.find("</body>").unwrap();
		(summary, html[start..end].trim().to_string())
	}
}

#[test]
fn environment_variables_are_dependencies() {
	let workspace = Workspace::new(
		"env",
		&[
			("nml.toml", ""),
			("a.nml", "Site: :lua`tostring(nml.env('NML_TEST_SITE'))`\n"),
			("b.nml", "No scripts.\n"),
		],
	);
	let unset = ["NML_TEST_SITE"];
	let (summary, body) = workspace.build(&[], &unset, "a.html");
	assert_eq!(summary, "2 parsed, 2 page(s) written, 0 failed");
	assert_eq!(body, "<p>Site: nil</p>");

	// Unchanged: nothing is parsed.
	let (summary, _) = workspace.build(&[], &unset, "a.html");
	assert_eq!(summary, "0 parsed, 0 page(s) written, 0 failed");

	// Set, then changed: only the page that reads it.
	let (summary, body) = workspace.build(&[("NML_TEST_SITE", "dev")], &[], "a.html");
	assert_eq!(summary, "1 parsed, 1 page(s) written, 0 failed");
	assert_eq!(body, "<p>Site: dev</p>");
	let (summary, _) = workspace.build(&[("NML_TEST_SITE", "dev")], &[], "a.html");
	assert_eq!(summary, "0 parsed, 0 page(s) written, 0 failed");
	let (summary, body) = workspace.build(&[("NML_TEST_SITE", "prod")], &[], "a.html");
	assert_eq!(summary, "1 parsed, 1 page(s) written, 0 failed");
	assert_eq!(body, "<p>Site: prod</p>");

	// Set to empty is not unset.
	let (summary, body) = workspace.build(&[("NML_TEST_SITE", "")], &[], "a.html");
	assert_eq!(summary, "1 parsed, 1 page(s) written, 0 failed");
	assert_eq!(body, "<p>Site: </p>");
	let (summary, _) = workspace.build(&[], &unset, "a.html");
	assert_eq!(summary, "1 parsed, 1 page(s) written, 0 failed");
}

#[test]
fn the_clock_is_read_again_on_every_build() {
	let workspace = Workspace::new(
		"clock",
		&[
			("nml.toml", ""),
			("a.nml", "Built in :lua`nml.date().year`.\n"),
			("b.nml", "No scripts.\n"),
		],
	);
	let unset = ["SOURCE_DATE_EPOCH"];
	let (summary, body) = workspace.build(&[], &unset, "a.html");
	assert_eq!(summary, "2 parsed, 2 page(s) written, 0 failed");
	assert!(body.starts_with("<p>Built in 20"), "{body}");
	let (summary, _) = workspace.build(&[], &unset, "a.html");
	assert_eq!(summary, "1 parsed, 1 page(s) written, 0 failed");

	// With `SOURCE_DATE_EPOCH`, the build is reproducible: parsed again only when it changes.
	let epoch = [("SOURCE_DATE_EPOCH", "0")];
	let (summary, body) = workspace.build(&epoch, &[], "a.html");
	assert_eq!(summary, "1 parsed, 1 page(s) written, 0 failed");
	assert_eq!(body, "<p>Built in 1970.</p>");
	let (summary, _) = workspace.build(&epoch, &[], "a.html");
	assert_eq!(summary, "0 parsed, 0 page(s) written, 0 failed");
	let (summary, body) = workspace.build(&[("SOURCE_DATE_EPOCH", "951782400")], &[], "a.html");
	assert_eq!(summary, "1 parsed, 1 page(s) written, 0 failed");
	assert_eq!(body, "<p>Built in 2000.</p>");

	// Unset again: the clock.
	let (summary, _) = workspace.build(&[], &unset, "a.html");
	assert_eq!(summary, "1 parsed, 1 page(s) written, 0 failed");
	let (summary, _) = workspace.build(&[], &unset, "a.html");
	assert_eq!(summary, "1 parsed, 1 page(s) written, 0 failed");
}
