use std::fs;
use std::path::PathBuf;

use nml_diagnostics::assert_diagnostics;

use super::*;

/// A temporary workspace, removed when dropped.
struct Workspace(PathBuf);

impl Drop for Workspace {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

impl Workspace {
	fn new(name: &str, files: &[(&str, &str)]) -> Self {
		let dir = std::env::temp_dir().join(format!("nml-build-{name}-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		let workspace = Self(dir);
		for (path, content) in files {
			workspace.write(path, content);
		}
		workspace
	}

	fn write(&self, path: &str, content: &str) {
		let path = self.0.join(path);
		fs::create_dir_all(path.parent().unwrap()).unwrap();
		fs::write(path, content).unwrap();
	}

	fn remove(&self, path: &str) {
		fs::remove_file(self.0.join(path)).unwrap();
	}

	fn output(&self, path: &str) -> String {
		fs::read_to_string(self.0.join("public").join(path)).unwrap()
	}

	fn build(&self) -> BuildReport {
		build(&self.0).unwrap()
	}
}

/// The body of a page, without the head.
fn body(page: &str) -> &str {
	let start = page.find("<body>\n").unwrap() + 7;
	let end = page.find("</body>").unwrap();
	&page[start..end]
}

#[test]
fn builds_pages() {
	let workspace = Workspace::new(
		"pages",
		&[
			(
				"nml.toml",
				"exclude = [\"lib/**\"]\n[html]\nlanguage = \"en\"\n",
			),
			("index.nml", "# Welcome {#top}\n\nSome **bold** text.\n"),
			("docs/guide.nml", "A *guide*."),
			("lib/util.nml", "not a page"),
			("broken.nml", "**unclosed"),
			(".hidden/x.nml", "hidden"),
		],
	);
	let report = workspace.build();

	assert_eq!(report.written, ["docs/guide.nml", "index.nml"]);
	assert_eq!(report.failed, ["broken.nml"]);
	assert_diagnostics!(report.diagnostics, [1001 @ 0..2]);

	let index = workspace.output("index.html");
	assert!(index.contains("<html lang=\"en\">"));
	assert!(index.contains("<title>Welcome</title>"));
	assert_eq!(
		body(&index),
		"<h1 id=\"top\">Welcome</h1>\n<p>Some <strong>bold</strong> text.</p>\n"
	);
	let guide = workspace.output("docs/guide.html");
	assert!(guide.contains("<title>guide</title>"));
	assert!(!workspace.0.join("public/broken.html").exists());
	assert!(!workspace.0.join("public/lib/util.html").exists());
	assert!(workspace.0.join(".nml/index.db").exists());
}

#[test]
fn needs_a_config() {
	let workspace = Workspace::new("config", &[("a.nml", "x")]);
	assert!(matches!(build(&workspace.0), Err(BuildError::NoConfig(_))));
}

#[test]
fn resolves_references() {
	let workspace = Workspace::new(
		"references",
		&[
			("nml.toml", ""),
			(
				"index.nml",
				"# Home {#top}\nSee [[guide#install]], [[#faq|the FAQ]], [[guide#]] and [[top]].\n",
			),
			(
				"docs/guide.nml",
				"# The guide\n## Installing {#install}\n[[index#top|Back]]\n## FAQ {#faq}\n",
			),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	assert_eq!(
		body(&workspace.output("index.html")),
		"<h1 id=\"top\">Home</h1>\n<p>See <a href=\"docs/guide.html#install\">Installing</a>, \
		<a href=\"docs/guide.html#faq\">the FAQ</a>, <a href=\"docs/guide.html\">The guide</a> \
		and <a href=\"#top\">Home</a>.</p>\n"
	);
	assert!(
		body(&workspace.output("docs/guide.html"))
			.contains("<p><a href=\"../index.html#top\">Back</a></p>")
	);
}

#[test]
fn relinks_only_what_changed() {
	let workspace = Workspace::new(
		"relink",
		&[
			("nml.toml", ""),
			("a.nml", "# A\n[[b#sec]]\n"),
			("b.nml", "# B\n## Section {#sec}\ntext\n"),
			("c.nml", "# C\nNo references.\n"),
		],
	);
	let first = workspace.build();
	assert_eq!(first.parsed, ["a.nml", "b.nml", "c.nml"]);
	assert_eq!(first.written, ["a.nml", "b.nml", "c.nml"]);

	// Nothing changed: nothing is parsed or written.
	let second = workspace.build();
	assert!(second.parsed.is_empty());
	assert!(second.written.is_empty());

	// A change that dependents cannot see: only the unit itself.
	workspace.write("b.nml", "# B\n## Section {#sec}\nother text\n");
	let report = workspace.build();
	assert_eq!(report.parsed, ["b.nml"]);
	assert_eq!(report.written, ["b.nml"]);

	// A new title for the symbol: `a` is relinked, without being parsed.
	workspace.write("b.nml", "# B\n## Renamed {#sec}\nother text\n");
	let report = workspace.build();
	assert_eq!(report.parsed, ["b.nml"]);
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	assert!(
		workspace
			.output("a.html")
			.contains("<a href=\"b.html#sec\">Renamed</a>")
	);

	// The symbol disappears: `a` shows an unresolved reference.
	workspace.write("b.nml", "# B\n");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	assert_diagnostics!(report.diagnostics, [3001 @ 4..13]);
	assert!(workspace.output("a.html").contains("nml-unresolved"));

	// It comes back.
	workspace.write("b.nml", "# B\n## Back {#sec}\n");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	assert_diagnostics!(report.diagnostics, []);
}

#[test]
fn unit_identity_changes() {
	let workspace = Workspace::new(
		"identity",
		&[
			("nml.toml", ""),
			("a.nml", "# A\n[[b#]] [[setup#]]\n"),
			("b.nml", "# Bee\n"),
			("c.nml", "# C\n"),
		],
	);
	workspace.build();

	// A new page title: users of the page are relinked.
	workspace.write("b.nml", "# Bee two\n");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	assert!(workspace.output("a.html").contains(">Bee two</a>"));

	// `c` takes the key `setup`: `[[setup#]]` now resolves.
	workspace.write("c.nml", ":set nml.reference_key = setup\n# C\n");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml", "c.nml"]);
	assert_diagnostics!(report.diagnostics, []);
	assert!(
		workspace
			.output("a.html")
			.contains("<a href=\"c.html\">C</a>")
	);

	// `b` is removed: `a` is relinked.
	workspace.remove("b.nml");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml"]);
	assert_diagnostics!(report.diagnostics, [3001]);
}

#[test]
fn ambiguous_references() {
	let workspace = Workspace::new(
		"ambiguous",
		&[
			("nml.toml", ""),
			("a.nml", "# A\n[[#x]] [[same#]]\n"),
			("b.nml", "# B {#x}\n"),
			("one/same.nml", "# One\n"),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);

	// A second `x` and a second unit named `same`.
	workspace.write("c.nml", "# C {#x}\n");
	workspace.write("two/same.nml", "# Two\n");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml", "c.nml", "two/same.nml"]);
	assert_diagnostics!(report.diagnostics, [3006 @ 4..10, 3006 @ 11..20]);
	assert_eq!(
		report.diagnostics[0].kind,
		DiagnosticKind::AmbiguousReference {
			target: "#x".into(),
			count: 2
		}
	);
}

#[test]
fn failed_units_keep_their_last_state() {
	let workspace = Workspace::new(
		"failed",
		&[
			("nml.toml", ""),
			("a.nml", "# A\n[[b#sec]]\n"),
			("b.nml", "# B\n## Section {#sec}\n"),
		],
	);
	workspace.build();

	// `b` breaks: its page and symbols stay as they were.
	workspace.write("b.nml", "# B\n## Section {#sec}\n**unclosed\n");
	let report = workspace.build();
	assert_eq!(report.failed, ["b.nml"]);
	assert!(report.written.is_empty());
	assert!(workspace.output("b.html").contains("Section"));

	// It is parsed again on every build until it is fixed.
	let report = workspace.build();
	assert_eq!(report.parsed, ["b.nml"]);
	workspace.write("b.nml", "# B\n## Fixed {#sec}\n");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	assert!(workspace.output("a.html").contains(">Fixed</a>"));
}

#[test]
fn links_to_libraries_are_unresolved() {
	let workspace = Workspace::new(
		"library",
		&[
			("nml.toml", "exclude = [\"lib/**\"]\n"),
			("a.nml", "[[util#x]] [[util#x|caption]]\n"),
			("lib/util.nml", "# Utilities {#x}\n"),
		],
	);
	let report = workspace.build();
	// The title resolves, but a library has no page to link to.
	assert_diagnostics!(report.diagnostics, [3001 @ 0..10, 3001 @ 11..29]);
	assert!(
		workspace
			.output("a.html")
			.contains("<a href=\"\">Utilities</a>")
	);
}

#[test]
fn link_errors_are_reported_until_fixed() {
	let workspace = Workspace::new(
		"link-errors",
		&[
			("nml.toml", ""),
			(
				"a.nml",
				"[[#missing]]
",
			),
			(
				"b.nml", "# B
",
			),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, [3001]);

	// Nothing changed, but `a` is linked again to report its error.
	let report = workspace.build();
	assert!(report.parsed.is_empty());
	assert_eq!(report.written, ["a.nml"]);
	assert_diagnostics!(report.diagnostics, [3001]);

	// Another unit defines the symbol: fixed, and then left alone.
	workspace.write(
		"b.nml",
		"# B {#missing}
",
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	let report = workspace.build();
	assert!(report.written.is_empty());
}

#[test]
fn configuration_changes_invalidate_everything() {
	let workspace = Workspace::new(
		"config-change",
		&[("nml.toml", ""), ("a.nml", "# A\n"), ("b.nml", "# B\n")],
	);
	workspace.build();
	assert!(workspace.build().parsed.is_empty());

	workspace.write("nml.toml", "[html]\nlanguage = \"fr\"\n");
	let report = workspace.build();
	assert_eq!(report.parsed, ["a.nml", "b.nml"]);
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	assert!(workspace.output("a.html").contains("<html lang=\"fr\">"));
	assert!(workspace.build().parsed.is_empty());
}

#[test]
fn dependents_are_invalidated_transitively() {
	use nml_workspace::FileState;

	let url = Url::parse("file:///w/nml.toml").unwrap();
	let config = Config::parse(Arc::new(Source::new(
		url,
		"[[module]]\nname = \"std\"\nroot = \"modules/std\"\n".into(),
		0,
	)))
	.unwrap();
	let mut index = Index::open_in_memory().unwrap();
	let mut add = |path: &str, dependencies: &[Dependency]| {
		index
			.commit(&StoredUnit {
				path,
				name: path,
				title: path,
				role: Role::Page,
				state: FileState {
					mtime: 0,
					size: 0,
					hash: Vec::new(),
				},
				exports: &[],
				uses: &[],
				dependencies,
				template: None,
			})
			.unwrap();
	};
	// a imports std.toc, which includes parts/x.nml. b includes a. c is independent.
	add(
		"modules/std/toc.nml",
		&[Dependency::Include("parts/x.nml".into())],
	);
	add("a.nml", &[Dependency::Import("std.toc".into())]);
	add("b.nml", &[Dependency::Include("a.nml".into())]);
	add("c.nml", &[Dependency::Config]);
	add("parts/x.nml", &[]);

	let existing: BTreeSet<&str> = [
		"a.nml",
		"b.nml",
		"c.nml",
		"modules/std/toc.nml",
		"parts/x.nml",
	]
	.into_iter()
	.collect();
	let run = |seeds: &[&str], existing: &BTreeSet<&str>| {
		let mut invalid: BTreeSet<String> = seeds.iter().map(|s| s.to_string()).collect();
		let seeds = invalid.iter().cloned().collect();
		invalidate(&index, &config, seeds, existing, &mut invalid).unwrap();
		invalid.into_iter().collect::<Vec<_>>()
	};
	assert_eq!(
		run(&["parts/x.nml"], &existing),
		["a.nml", "b.nml", "modules/std/toc.nml", "parts/x.nml"]
	);
	assert_eq!(run(&["a.nml"], &existing), ["a.nml", "b.nml"]);
	assert_eq!(run(&["b.nml"], &existing), ["b.nml"]);
	assert_eq!(run(&["c.nml"], &existing), ["c.nml"]);

	// A removed file is a seed, but only existing files are parsed.
	let without_a: BTreeSet<&str> = existing.iter().copied().filter(|p| *p != "a.nml").collect();
	assert_eq!(
		run(&["modules/std/toc.nml"], &without_a),
		["modules/std/toc.nml"]
	);
}

#[test]
fn includes_and_imports_invalidate_their_users() {
	let workspace = Workspace::new(
		"includes",
		&[
			(
				"nml.toml",
				"exclude = [\"parts/**\", \"modules/**\"]\n[[module]]\nname = \"std\"\nroot = \"modules/std\"\n",
			),
			("a.nml", ":import std.toc\n# A\n:include parts/intro\n"),
			("b.nml", "# B\n[[a#intro]]\n"),
			("c.nml", "# C\n"),
			("parts/intro.nml", "## Intro {#intro}\n"),
			("modules/std/toc.nml", "toc\n"),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	assert!(
		workspace
			.output("a.html")
			.contains("<h2 id=\"intro\">Intro</h2>")
	);
	assert!(
		workspace
			.output("b.html")
			.contains("<a href=\"a.html#intro\">Intro</a>")
	);

	// The included file changes: `a` is parsed again, and `b` relinked for the new title.
	workspace.write("parts/intro.nml", "## Introduction {#intro}\n");
	let report = workspace.build();
	assert_eq!(report.parsed, ["a.nml", "parts/intro.nml"]);
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	assert!(workspace.output("b.html").contains(">Introduction</a>"));

	// The module changes: its importer is parsed again.
	workspace.write("modules/std/toc.nml", "new toc\n");
	let report = workspace.build();
	assert_eq!(report.parsed, ["a.nml", "modules/std/toc.nml"]);
	assert_eq!(report.written, ["a.nml"]);

	// The included file is removed: `a` fails, and keeps its last valid state.
	workspace.remove("parts/intro.nml");
	let report = workspace.build();
	assert_eq!(report.failed, ["a.nml"]);
	assert_diagnostics!(report.diagnostics, [3008]);
}

#[test]
fn outputs_of_removed_pages_are_deleted() {
	let workspace = Workspace::new(
		"collect",
		&[
			("nml.toml", ""),
			("a.nml", "# A\n"),
			("docs/deep/b.nml", "# B\n"),
			("docs/c.nml", "# C\n"),
		],
	);
	workspace.build();
	// A file of the user's in the output directory.
	workspace.write("public/style.css", "p {}");

	workspace.remove("docs/deep/b.nml");
	let report = workspace.build();
	assert_eq!(report.deleted, ["docs/deep/b.nml"]);
	assert!(!workspace.0.join("public/docs/deep").exists());
	assert!(workspace.0.join("public/docs/c.html").exists());

	// `docs/c` becomes a library: its output goes, and `docs/` with it.
	workspace.write("nml.toml", "exclude = [\"docs/**\"]\n");
	let report = workspace.build();
	assert_eq!(report.deleted, ["docs/c.nml"]);
	assert!(!workspace.0.join("public/docs").exists());
	assert!(workspace.0.join("public/a.html").exists());
	assert!(workspace.0.join("public/style.css").exists());

	// Removing a library deletes nothing.
	workspace.remove("docs/c.nml");
	assert!(workspace.build().deleted.is_empty());
}

#[test]
fn lua() {
	let workspace = Workspace::new(
		"lua",
		&[
			("nml.toml", ""),
			(
				"a.nml",
				":lua\n```\nfunction greet(name) return 'Hello, ' .. name end\n```\n\n:lua`greet('NML')`!\n",
			),
			("b.nml", "x :lua`nil + 1`\n"),
		],
	);
	let report = workspace.build();
	assert_eq!(report.failed, ["b.nml"]);
	assert_diagnostics!(report.diagnostics, [4001]);
	let location = report.diagnostics[0].primary().location.root();
	assert_eq!((location.range(), location.content()), (7..14, "nil + 1"));
	assert!(workspace.output("a.html").contains("<p>Hello, NML!</p>"));
}

#[test]
fn module_lua_reaches_its_importers() {
	let workspace = Workspace::new(
		"module-lua",
		&[
			(
				"nml.toml",
				"exclude = [\"modules/**\"]\n[[module]]\nname = \"std\"\nroot = \"modules/std\"\n",
			),
			("a.nml", ":import std.util\n\n:lua`util.name`\n"),
			("modules/std/util.nml", ":lua\n```\nname = 'first'\n```\n"),
		],
	);
	assert_diagnostics!(workspace.build().diagnostics, []);
	assert!(workspace.output("a.html").contains("<p>first</p>"));

	workspace.write("modules/std/util.nml", ":lua\n```\nname = 'second'\n```\n");
	let report = workspace.build();
	assert_eq!(report.parsed, ["a.nml", "modules/std/util.nml"]);
	assert!(workspace.output("a.html").contains("<p>second</p>"));
}

#[test]
fn files_read_by_scripts_are_dependencies() {
	let workspace = Workspace::new(
		"read",
		&[
			("nml.toml", ""),
			(
				"a.nml",
				"Rows: :lua`select(2, nml.read('data.csv'):gsub('\\n', ''))`\n",
			),
			("b.nml", "# B\n"),
			("data.csv", "x\ny\n"),
		],
	);
	assert_diagnostics!(workspace.build().diagnostics, []);
	assert!(workspace.output("a.html").contains("<p>Rows: 2</p>"));
	assert!(workspace.build().parsed.is_empty());

	// The CSV changes: only its reader is parsed again.
	workspace.write("data.csv", "x\ny\nz\n");
	let report = workspace.build();
	assert_eq!(report.parsed, ["a.nml"]);
	assert!(workspace.output("a.html").contains("<p>Rows: 3</p>"));

	// It disappears: the reader fails, and keeps its last state.
	workspace.remove("data.csv");
	let report = workspace.build();
	assert_eq!(report.failed, ["a.nml"]);
	assert_diagnostics!(report.diagnostics, [4001]);
}

#[test]
fn navigation_from_deferred_slots() {
	let nav = r#":lua
```
function menu()
	local items = { nml.element('raw', { content = '<nav>' }) }
	for i, page in ipairs(nml.pages()) do
		if i > 1 then table.insert(items, ' ') end
		table.insert(items, nml.element('link', { url = page.url, page.title }))
	end
	table.insert(items, nml.element('raw', { content = '</nav>' }))
	return items
end
```
"#;
	let workspace = Workspace::new(
		"navigation",
		&[
			(
				"nml.toml",
				"exclude = [\"modules/**\"]\n[[module]]\nname = \"std\"\nroot = \"modules/std\"\n",
			),
			(
				"index.nml",
				":import std.nav\n# Home\n:lua`nml.defer('std.nav', 'menu')`\n",
			),
			("docs/guide.nml", "# Guide\n"),
			("modules/std/nav.nml", nav),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	assert!(
		workspace.output("index.html").contains(
			r#"<nav><a href="docs/guide.html">Guide</a> <a href="index.html">Home</a></nav>"#
		),
		"{}",
		workspace.output("index.html")
	);

	// A new page: the page with the menu is relinked, not parsed.
	workspace.write("about.nml", "# About\n");
	let report = workspace.build();
	assert_eq!(report.parsed, ["about.nml"]);
	assert_eq!(report.written, ["about.nml", "index.nml"]);
	assert!(
		workspace
			.output("index.html")
			.contains(r#"<a href="about.html">About</a>"#)
	);

	// A new title, likewise.
	workspace.write("docs/guide.nml", "# User guide\n");
	let report = workspace.build();
	assert_eq!(report.written, ["docs/guide.nml", "index.nml"]);
	assert!(workspace.output("index.html").contains(">User guide</a>"));

	// A change pages cannot see: only the page itself.
	workspace.write("docs/guide.nml", "# User guide\ntext\n");
	assert_eq!(workspace.build().written, ["docs/guide.nml"]);
}

#[test]
fn builds_without_writing() {
	let workspace = Workspace::new(
		"no-write",
		&[
			("nml.toml", ""),
			("a.nml", "[[#missing]]\n"),
			("b.nml", "# B\n"),
		],
	);
	let report = build_with(&workspace.0, BuildOptions { write: false }).unwrap();
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	assert_diagnostics!(report.diagnostics, [3001]);
	assert!(!workspace.0.join("public").exists());

	// The index is up to date: a build that writes only writes what is relinked.
	workspace.remove("b.nml");
	let report = build_with(&workspace.0, BuildOptions { write: false }).unwrap();
	assert!(report.deleted.is_empty());
	assert!(!workspace.0.join("public").exists());
}

#[test]
fn overlays_replace_files() {
	let workspace = Workspace::new("overlay", &[("nml.toml", ""), ("part.nml", "on disk\n")]);
	let resolver = WorkspaceResolver::new(workspace.0.clone(), Vec::new());
	let (_, source) = resolver.include("a.nml", "part").unwrap();
	assert_eq!(source.content(), "on disk\n");

	let overlay = [("part.nml".to_string(), "in memory\n".to_string())].into();
	let resolver = resolver.with_overlay(overlay);
	let (_, source) = resolver.include("a.nml", "part").unwrap();
	assert_eq!(source.content(), "in memory\n");
	assert_eq!(resolver.read("a.nml", "part.nml").unwrap().1, "in memory\n");
}

#[test]
fn code_stylesheet() {
	let workspace = Workspace::new(
		"code-stylesheet",
		&[
			("nml.toml", ""),
			("index.nml", "No code."),
			("docs/a.nml", "```rust\nfn main() {}\n```\n"),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	let link = "<link rel=\"stylesheet\" href=\"../nml-code.css\">";
	assert!(workspace.output("docs/a.html").contains(link));
	assert!(!workspace.output("index.html").contains("nml-code.css"));
	let css = workspace.output("nml-code.css");
	assert!(css.contains(".hl-"), "{css}");

	// A new theme rewrites the stylesheet.
	workspace.write("nml.toml", "[html]\ncode_theme = \"InspiredGitHub\"\n");
	workspace.build();
	assert_ne!(workspace.output("nml-code.css"), css);
}

#[test]
fn assets_are_copied() {
	let workspace = Workspace::new(
		"assets",
		&[
			("nml.toml", ""),
			("docs/a.nml", ":figure #f Flower\n![](../img/flower.png)\n"),
			("img/flower.png", "v1"),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	assert!(
		workspace
			.output("docs/a.html")
			.contains("<img src=\"../img/flower.png\" alt=\"\">")
	);
	assert_eq!(workspace.output("img/flower.png"), "v1");

	// A new version of the file is copied: the page depends on it.
	workspace.write("img/flower.png", "version 2");
	let report = workspace.build();
	assert_eq!(report.parsed, ["docs/a.nml"]);
	assert_eq!(workspace.output("img/flower.png"), "version 2");

	// A missing file is an error.
	workspace.remove("img/flower.png");
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, [3008]);
}
