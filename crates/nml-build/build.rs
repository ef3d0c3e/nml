//! `NML_BUILD_ID`: what a build's output depends on besides the workspace, the version and,
//! in a source tree, a hash of the crates that make pages. An index written with another
//! identity is rebuilt, so that pages pick up a new writer or stylesheet.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

/// The crates whose changes do not change pages.
const SKIPPED: [&str; 3] = ["nml-cli", "nml-lsp", "nml-preview"];

fn main() {
	let version = std::env::var("CARGO_PKG_VERSION").unwrap();
	let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
	let crates = manifest.parent().unwrap();
	let mut files = Vec::new();
	// Published alone, there are no other crates: the version is the identity.
	if crates.join("nml-html").is_dir() {
		for entry in fs::read_dir(crates).unwrap() {
			let path = entry.unwrap().path();
			let name = path.file_name().unwrap().to_string_lossy().into_owned();
			if !path.is_dir() || SKIPPED.contains(&name.as_str()) {
				continue;
			}
			println!("cargo:rerun-if-changed={}", path.display());
			walk(&path, &mut files);
		}
	}
	files.sort();
	// FNV-1a: stable across Rust versions, unlike the standard library's hasher.
	let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
	for file in &files {
		let relative = file.strip_prefix(crates).unwrap().to_string_lossy();
		let content = fs::read(file).unwrap();
		for byte in relative
			.as_bytes()
			.iter()
			.chain([0u8].iter())
			.chain(&content)
		{
			hash ^= u64::from(*byte);
			hash = hash.wrapping_mul(0x0100_0000_01b3);
		}
	}
	println!("cargo:rustc-env=NML_BUILD_ID={version}-{hash:016x}");
}

fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
	for entry in fs::read_dir(dir).unwrap() {
		let path = entry.unwrap().path();
		if path.is_dir() {
			if path.file_name().is_some_and(|name| name == "target") {
				continue;
			}
			walk(&path, files);
		} else {
			files.push(path);
		}
	}
}
