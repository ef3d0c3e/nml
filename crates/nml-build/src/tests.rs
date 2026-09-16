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

/// The content of a page, without the page around it.
fn body(page: &str) -> &str {
	let start =
		page.find("<main class=\"nml-main\">\n").unwrap() + "<main class=\"nml-main\">\n".len();
	let end = page.find("</main>").unwrap();
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
		"<h1 id=\"top\"><a class=\"heading-link\" href=\"#top\">🔗</a> <span class=\"heading-number\">1.</span> Welcome</h1>\n<p>Some <strong>bold</strong> text.</p>\n"
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
		"<h1 id=\"top\"><a class=\"heading-link\" href=\"#top\">🔗</a> <span class=\"heading-number\">1.</span> Home</h1>\n<p>See <a href=\"docs/guide.html#install\">Installing</a>, \
		<a href=\"docs/guide.html#faq\">the FAQ</a>, <a href=\"docs/guide.html\">The guide</a> \
		and <a href=\"#top\">Home</a>.</p>\n"
	);
	assert!(
		body(&workspace.output("docs/guide.html"))
			.contains("<p><a href=\"../index.html#top\">Back</a></p>")
	);
}

#[test]
fn media_references() {
	let workspace = Workspace::new(
		"media-references",
		&[
			("nml.toml", ""),
			("cat.png", "cat"),
			(
				"a.nml",
				"![](cat.png) ![A cat](cat.png){#cat}\n\nSee [[cat]] and [[cat|the {title} {label}]].\n",
			),
			("b.nml", "[[a#cat]]\n"),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	let asset = copy_of("cat.png", "cat");
	assert_eq!(
		body(&workspace.output("a.html")),
		format!(
			"<p><img src=\"{asset}\" alt=\"\"> <img id=\"cat\" src=\"{asset}\" alt=\"A cat\">\
			<span class=\"media-number\">(1)</span></p>\n\
			<p>See <a href=\"#cat\">(1)</a> and <a href=\"#cat\">the A cat (1)</a>.</p>\n"
		)
	);
	assert_eq!(
		body(&workspace.output("b.html")),
		"<p><a href=\"a.html#cat\">(1)</a></p>\n"
	);
}

#[test]
fn reference_labels() {
	let workspace = Workspace::new(
		"labels",
		&[
			("nml.toml", ""),
			(
				"a.nml",
				"# A\n## Setup {#setup}\n:anchor here Here\n\n:figure #f1 One\nx\n\n:figure #f2 Two\ny\n\n:table #t Prices\n| a |\n",
			),
			(
				"b.nml",
				"[[a#f2]], [[a#t]], [[a#setup]], [[a#here]], [[a#]], [[a#f2|caption]].\n\n[[a#f2|see figure **{number}**: {title}]] [[a#setup|{label}, {number}]]\n",
			),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	// Figures and tables by their number, anything else by its title.
	assert_eq!(
		body(&workspace.output("b.html")),
		"<p><a href=\"a.html#f2\">Figure 2</a>, <a href=\"a.html#t\">Table 1</a>, \
		<a href=\"a.html#setup\">Setup</a>, <a href=\"a.html#here\">Here</a>, \
		<a href=\"a.html\">A</a>, <a href=\"a.html#f2\">caption</a>.</p>\n\
		<p><a href=\"a.html#f2\">see figure <strong>2</strong>: Two</a> \
		<a href=\"a.html#setup\">Setup, </a></p>\n"
	);

	// A figure added before it: the label follows, without parsing `b`.
	workspace.write(
		"a.nml",
		"# A\n## Setup {#setup}\n:anchor here Here\n\n:figure #f0 Zero\nw\n\n:figure #f1 One\nx\n\n:figure #f2 Two\ny\n\n:table #t Prices\n| a |\n",
	);
	let report = workspace.build();
	assert_eq!(report.parsed, ["a.nml"]);
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	let b = body(&workspace.output("b.html")).to_string();
	assert!(b.contains(">Figure 3</a>"), "{b}");
	assert!(b.contains(">see figure <strong>3</strong>: Two</a>"), "{b}");
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
fn relinks_when_urls_change() {
	let workspace = Workspace::new(
		"urls",
		&[
			("nml.toml", ""),
			("a.nml", "# A\n[[#sec]]\n"),
			("b.nml", "# B\n## S {#sec}\n"),
			("c.nml", "# C\n"),
		],
	);
	workspace.build();
	assert!(workspace.output("a.html").contains("href=\"b.html#sec\""));

	// The symbol moves to another page: `a` is relinked, not parsed.
	workspace.write("b.nml", "# B\n");
	workspace.write("c.nml", "# C\n## S {#sec}\n");
	let report = workspace.build();
	assert_eq!(report.parsed, ["b.nml", "c.nml"]);
	assert_eq!(report.written, ["a.nml", "b.nml", "c.nml"]);
	assert_diagnostics!(report.diagnostics, []);
	assert!(workspace.output("a.html").contains("href=\"c.html#sec\""));

	// Its page moves: only the URL changes, and `a` is relinked.
	workspace.write("c.nml", ":set nml.output = x/c.html\n\n# C\n## S {#sec}\n");
	let report = workspace.build();
	assert_eq!(report.parsed, ["c.nml"]);
	assert_eq!(report.written, ["a.nml", "c.nml"]);
	assert!(workspace.output("a.html").contains("href=\"x/c.html#sec\""));

	// The using page moves: its URLs are relative to where it is written.
	workspace.write("a.nml", ":set nml.output = y/a.html\n\n# A\n[[#sec]]\n");
	let report = workspace.build();
	assert_eq!(report.written, ["a.nml"]);
	assert!(
		workspace
			.output("y/a.html")
			.contains("href=\"../x/c.html#sec\"")
	);
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
				output: "a.html",
				output_range: None,
				assets: &[],
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
			.contains("<span class=\"heading-number\">1.1.</span> Intro</h2>")
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

/// A build that does not write, as the language server's.
fn editor_build(workspace: &Workspace) -> BuildReport {
	build_with(&workspace.0, BuildOptions { write: false }).unwrap()
}

#[test]
fn builds_that_do_not_write_leave_pages_to_write() {
	let workspace = Workspace::new(
		"unwritten",
		&[
			("nml.toml", ""),
			("foo.nml", "# Foo\n"),
			("b.nml", "# B {#x}\n"),
		],
	);
	workspace.build();
	workspace.write("a.nml", "See [[b#x]].\n");
	workspace.build();

	// The renamed page is written, and the old output deleted.
	fs::rename(workspace.0.join("foo.nml"), workspace.0.join("bar.nml")).unwrap();
	let report = workspace.build();
	assert_eq!(report.deleted, ["foo.nml"]);
	assert!(!workspace.0.join("public/foo.html").exists());

	// A new page, an edited page and a page to relink, all seen by the editor first.
	workspace.write("foo.nml", "# New foo\n");
	workspace.write("b.nml", "# Bee {#x}\n");
	let report = editor_build(&workspace);
	assert_eq!(report.written, ["a.nml", "b.nml", "foo.nml"]);
	assert!(!workspace.0.join("public/foo.html").exists());
	// Building again without writing changes nothing.
	editor_build(&workspace);

	let report = workspace.build();
	assert_eq!(report.written, ["a.nml", "b.nml", "foo.nml"]);
	assert!(workspace.output("foo.html").contains("New foo</h1>"));
	assert!(workspace.output("b.html").contains("Bee"));
	assert!(
		workspace.output("a.html").contains(">Bee</a>"),
		"{}",
		workspace.output("a.html")
	);
	// Then they are up to date.
	assert!(workspace.build().written.is_empty());
}

#[test]
fn builds_that_do_not_write_leave_outputs_to_delete() {
	let workspace = Workspace::new(
		"stale",
		&[("nml.toml", ""), ("a.nml", "# A\n"), ("b.nml", "# B\n")],
	);
	workspace.build();

	// Removed while only the editor builds: deleted by the next build that writes.
	workspace.remove("a.nml");
	let report = editor_build(&workspace);
	assert!(report.deleted.is_empty());
	assert!(workspace.0.join("public/a.html").exists());
	let report = workspace.build();
	assert_eq!(report.deleted, ["a.nml"]);
	assert!(!workspace.0.join("public/a.html").exists());
	assert!(workspace.build().deleted.is_empty());

	// Removed, then made again: the output is the new page's, and is not deleted.
	workspace.remove("b.nml");
	editor_build(&workspace);
	workspace.write("b.nml", "# New B\n");
	editor_build(&workspace);
	let report = workspace.build();
	assert!(report.deleted.is_empty());
	assert_eq!(report.written, ["b.nml"]);
	assert!(workspace.output("b.html").contains("New B"));
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
			("index.nml", "No code, `plain` code."),
			("docs/a.nml", "```rust\nfn main() {}\n```\n"),
			("docs/b.nml", "Inline :code[rust]`fn main() {}`.\n"),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	let link = "<link rel=\"stylesheet\" href=\"../nml-code.css\">";
	assert!(workspace.output("docs/a.html").contains(link));
	// Highlighted inline code needs it too; plain code does not.
	assert!(workspace.output("docs/b.html").contains(link));
	assert!(!workspace.output("index.html").contains("nml-code.css"));
	let css = workspace.output("nml-code.css");
	assert!(css.contains(".hl-"), "{css}");

	// A new theme rewrites the stylesheet, light or dark.
	workspace.write("nml.toml", "[html]\ncode_theme = \"Solarized (light)\"\n");
	workspace.build();
	let light = workspace.output("nml-code.css");
	assert_ne!(light, css);
	workspace.write(
		"nml.toml",
		"[html]\ncode_theme = \"Solarized (light)\"\ncode_theme_dark = \"Solarized (dark)\"\n",
	);
	workspace.build();
	let dark = workspace.output("nml-code.css");
	assert_ne!(dark, light);
	// The light colors stay.
	assert_eq!(dark.split("@media").next(), light.split("@media").next());
}

/// Where the workspace file `path` with this content is copied, in the output directory.
fn copy_of(path: &str, content: &str) -> String {
	nml_lang::asset_output_path(path, &hex(&nml_workspace::hash(content.as_bytes())))
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
	let v1 = copy_of("img/flower.png", "v1");
	assert!(v1.starts_with("assets/") && v1.ends_with(".png"), "{v1}");
	assert!(
		workspace
			.output("docs/a.html")
			.contains(&format!("<img src=\"../{v1}\" alt=\"\">"))
	);
	assert_eq!(workspace.output(&v1), "v1");
	assert!(!workspace.0.join("public/img").exists());

	// A new version of the file gets a new name, and the old copy goes.
	workspace.write("img/flower.png", "version 2");
	let report = workspace.build();
	assert_eq!(report.parsed, ["docs/a.nml"]);
	let v2 = copy_of("img/flower.png", "version 2");
	assert_ne!(v1, v2);
	assert!(
		workspace
			.output("docs/a.html")
			.contains(&format!("<img src=\"../{v2}\""))
	);
	assert_eq!(workspace.output(&v2), "version 2");
	assert!(!workspace.0.join("public").join(&v1).exists());

	// A copy that went away is copied again, though no page changed.
	fs::remove_dir_all(workspace.0.join("public/assets")).unwrap();
	let report = workspace.build();
	assert!(report.parsed.is_empty());
	assert_eq!(workspace.output(&v2), "version 2");

	// A missing file is an error.
	workspace.remove("img/flower.png");
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, [3008]);
}

#[test]
fn figures_from_scripts() {
	let workspace = Workspace::new(
		"script-figures",
		&[
			("nml.toml", ""),
			(
				"docs/a.nml",
				"# A\n\n:figure #first First\nx\n\n:lua`nml.element('figure', {ref = 'shot', caption = 'A *shot*', nml.element('media', {url = '../img/shot.png'})})`\n",
			),
			("b.nml", "See [[a#shot]]."),
			("img/shot.png", "png"),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	let shot = copy_of("img/shot.png", "png");
	assert_eq!(workspace.output(&shot), "png");
	assert!(
		workspace
			.output("docs/a.html")
			.contains(&format!("<img src=\"../{shot}\" alt=\"\">"))
	);
	let b = body(&workspace.output("b.html")).to_string();
	assert!(
		b.contains("<a href=\"docs/a.html#shot\">Figure 2</a>"),
		"{b}"
	);

	// The image changes: the page is parsed again, and its copy renamed.
	workspace.write("img/shot.png", "png 2");
	let report = workspace.build();
	assert_eq!(report.parsed, ["docs/a.nml"]);
	assert_eq!(workspace.output(&copy_of("img/shot.png", "png 2")), "png 2");
}

#[test]
fn unused_assets_are_deleted() {
	let workspace = Workspace::new(
		"unused-assets",
		&[
			("nml.toml", "exclude = [\"lib.nml\"]\n"),
			("a.nml", "![](img/a.png) ![](img/shared.png)"),
			("b.nml", "![](img/shared.png) ![](other/same.png)"),
			("lib.nml", "![](img/lib.png)"),
			("img/a.png", "a"),
			("img/shared.png", "shared"),
			("other/same.png", "shared"),
			("img/lib.png", "lib"),
		],
	);
	assert_diagnostics!(workspace.build().diagnostics, []);
	let output = workspace.0.join("public");
	let (a, shared) = (
		copy_of("img/a.png", "a"),
		copy_of("img/shared.png", "shared"),
	);
	// Files with the same content share a copy.
	assert_eq!(shared, copy_of("other/same.png", "shared"));
	assert!(output.join(&a).exists());
	assert!(output.join(&shared).exists());
	// Only pages are written, and only their media copied.
	assert!(!output.join(copy_of("img/lib.png", "lib")).exists());
	assert_eq!(fs::read_dir(output.join("assets")).unwrap().count(), 2);

	// Files of the assets directory no page shows are deleted, with directories.
	workspace.write("public/assets/old/x.png", "old");
	workspace.write("a.nml", "![](img/shared.png)");
	assert_diagnostics!(workspace.build().diagnostics, []);
	assert!(!output.join(&a).exists());
	assert!(!output.join("assets/old").exists());
	assert!(output.join(&shared).exists());

	// Still shown by the other file with that content.
	workspace.remove("a.nml");
	workspace.write("b.nml", "![](other/same.png)");
	assert_diagnostics!(workspace.build().diagnostics, []);
	assert!(output.join(&shared).exists());

	// The last page that shows a file.
	workspace.remove("b.nml");
	workspace.build();
	assert_eq!(fs::read_dir(output.join("assets")).unwrap().count(), 0);
}

#[test]
fn output_paths() {
	let workspace = Workspace::new(
		"output-paths",
		&[
			("nml.toml", ""),
			(
				"docs/install.nml",
				":set nml.output = guide/setup/index.html\n\n# Install {#install}\n\n![](../img/a.png)\n\n```rust\nfn main() {}\n```\n",
			),
			("index.nml", "See [[install#install]] and [[install#]]."),
			("img/a.png", "a"),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	assert!(!workspace.0.join("public/docs").exists());
	let page = workspace.output("guide/setup/index.html");
	// URLs in the page are relative to where it is written.
	let image = format!("<img src=\"../../{}\"", copy_of("img/a.png", "a"));
	assert!(page.contains(&image), "{page}");
	assert!(page.contains("href=\"../../nml-code.css\""), "{page}");
	let index = workspace.output("index.html");
	assert!(
		index.contains("href=\"guide/setup/index.html#install\""),
		"{index}"
	);
	assert!(index.contains("href=\"guide/setup/index.html\""), "{index}");

	// Moving the page deletes its old output, with the directories it leaves empty, and
	// relinks the pages that refer to it.
	workspace.write(
		"docs/install.nml",
		":set nml.output = install.html\n\n# Install {#install}\n",
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	assert_eq!(report.written, ["docs/install.nml", "index.nml"]);
	assert!(!workspace.0.join("public/guide").exists());
	assert!(
		workspace
			.output("index.html")
			.contains("href=\"install.html#install\"")
	);

	// Without the variable, the page is written at its path.
	workspace.write("docs/install.nml", "# Install {#install}\n");
	let report = workspace.build();
	assert_eq!(report.written, ["docs/install.nml", "index.nml"]);
	assert!(!workspace.0.join("public/install.html").exists());
	assert!(workspace.0.join("public/docs/install.html").exists());
	assert!(
		workspace
			.output("index.html")
			.contains("href=\"docs/install.html#install\"")
	);
}

#[test]
fn configured_urls_are_relative_to_each_page() {
	let workspace = Workspace::new(
		"configured-urls",
		&[
			(
				"nml.toml",
				"[html]\nstylesheets = [\"style.css\", \"/root.css\", \"https://cdn.example/a.css\"]\nicon = \"img/icon.png\"\n",
			),
			("index.nml", "a"),
			("docs/guide.nml", "b"),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	let head = |page: &str| {
		let page = workspace.output(page);
		page[page.find("<link").unwrap()..page.find("</head>").unwrap()].to_string()
	};
	let links = |prefix: &str| {
		format!(
			"<link rel=\"stylesheet\" href=\"{prefix}style.css\">\n\
			<link rel=\"stylesheet\" href=\"/root.css\">\n\
			<link rel=\"stylesheet\" href=\"https://cdn.example/a.css\">\n\
			<link rel=\"icon\" href=\"{prefix}img/icon.png\">\n"
		)
	};
	assert_eq!(head("index.html"), links(""));
	assert_eq!(head("docs/guide.html"), links("../"));
}

#[test]
fn invalid_output_paths() {
	for value in ["../x.html", "/x.html", "x", "assets/x.html", "a//b.html"] {
		let workspace = Workspace::new(
			"invalid-output",
			&[
				("nml.toml", ""),
				("a.nml", &format!(":set nml.output = {value}\n\nA")),
			],
		);
		let report = workspace.build();
		let end = 18 + value.len();
		assert_diagnostics!(report.diagnostics, [2021 @ 18..end]);
		assert_eq!(report.failed, ["a.nml"], "{value}");
	}
}

#[test]
fn output_conflicts() {
	let workspace = Workspace::new(
		"output-conflicts",
		&[
			("nml.toml", ""),
			("a.nml", "A"),
			("b.nml", ":set nml.output = a.html\n\nB"),
			("c.nml", "C"),
		],
	);
	let report = workspace.build();
	// Neither page is written; the error is at `nml.output` where it is set.
	let messages: Vec<_> = report
		.diagnostics
		.iter()
		.map(|d| {
			let primary = d.primary();
			(
				d.kind.clone(),
				primary
					.location
					.source()
					.location()
					.path()
					.rsplit('/')
					.next()
					.unwrap()
					.to_string(),
				primary.location.range(),
			)
		})
		.collect();
	assert_eq!(
		messages,
		[
			(
				DiagnosticKind::OutputConflict {
					output: "a.html".into(),
					other: "b.nml".into(),
				},
				"a.nml".into(),
				0..0
			),
			(
				DiagnosticKind::OutputConflict {
					output: "a.html".into(),
					other: "a.nml".into(),
				},
				"b.nml".into(),
				18..24
			),
		]
	);
	assert!(report.has_errors());
	assert_eq!(report.written, ["c.nml"]);
	assert!(!workspace.0.join("public/a.html").exists());

	// Reported at each build until it is fixed.
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, [3013, 3013]);
	assert!(report.written.is_empty());

	// Then both pages are written, though only one changed.
	workspace.write("b.nml", ":set nml.output = b.html\n\nB");
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	assert_eq!(report.written, ["a.nml", "b.nml"]);
	assert!(body(&workspace.output("a.html")).contains('A'));
	assert!(body(&workspace.output("b.html")).contains('B'));

	// A page written where another page's default output is.
	workspace.write("x/y.nml", "Y");
	workspace.write("z.nml", ":set nml.output = x/y.html\n\nZ");
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, [3013, 3013]);
}

#[test]
fn std_modules() {
	let workspace = Workspace::new(
		"std",
		&[
			("nml.toml", ""),
			(
				"index.nml",
				":import std.nav\n:export nav.order = 1\n\n# Home\n:nav\n",
			),
			("docs/guide.nml", ":export nav.category = Docs\n# Guide\n"),
			(
				"docs/faq.nml",
				":export nav.category = Docs\n:export nav.order = 1\n# FAQ\n",
			),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	let menu = "<nav class=\"nav\">\n<ul>\n\
		<li class=\"current\"><a href=\"index.html\">Home</a></li>\n\
		<li class=\"category\">Docs<ul>\n\
		<li><a href=\"docs/faq.html\">FAQ</a></li>\n\
		<li><a href=\"docs/guide.html\">Guide</a></li>\n\
		</ul>\n</li>\n</ul>\n</nav>\n";
	assert!(
		workspace.output("index.html").contains(menu),
		"{}",
		workspace.output("index.html")
	);

	// A page's navigation variables are part of what menus show.
	workspace.write("docs/guide.nml", ":export nav.category = Guides\n# Guide\n");
	let report = workspace.build();
	assert_eq!(report.written, ["docs/guide.nml", "index.nml"]);
	assert!(
		workspace
			.output("index.html")
			.contains("<li class=\"category\">Guides<ul>")
	);

	// An unknown module of `std`.
	workspace.write("index.nml", ":import std.nope\n");
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, [3008]);
}

#[test]
fn rendered_graphs() {
	if std::process::Command::new("dot")
		.arg("-V")
		.output()
		.is_err()
	{
		eprintln!("skipped: no `dot`");
		return;
	}
	let workspace = Workspace::new(
		"render-graph",
		&[
			("nml.toml", ""),
			(
				"a.nml",
				":figure #g Stages\n:graph[layout=dot]\n```dot\ndigraph { scan -> parse }\n```\n",
			),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	let page = workspace.output("a.html");
	assert!(
		page.contains(
			"<div class=\"rendered graph\" role=\"img\" aria-label=\"digraph { scan -&gt; parse }\"><svg"
		),
		"{page}"
	);
	assert!(page.contains("<figcaption><span class=\"figure-number\">Figure 1:</span> Stages"));

	// The SVG comes from the cache when the page changes elsewhere.
	workspace.write(
		"a.nml",
		"Intro.\n\n:figure #g Stages\n:graph[layout=dot]\n```dot\ndigraph { scan -> parse }\n```\n",
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	assert!(workspace.output("a.html").contains("<svg"));

	// A graph that does not render is an error of the page.
	workspace.write("a.nml", ":graph\n```\ndigraph { a -> }\n```\n");
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, [3012 @ 0..32]);
	assert_eq!(report.failed, ["a.nml"]);
}

#[test]
fn missing_renderers() {
	let workspace = Workspace::new(
		"render-missing",
		&[
			("nml.toml", "[render]\nlatex = \"nml-no-such-latex\"\n"),
			("a.nml", "Math: $x$.\n"),
			("b.nml", ":tex[env=nope]\n```\nx\n```\n"),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, [3012 @ 6..9, 3012 @ 0..25]);
	let message = format!("{:?}", report.diagnostics[0].kind);
	assert!(
		message.contains("cannot run `nml-no-such-latex`"),
		"{message}"
	);
	let message = format!("{:?}", report.diagnostics[1].kind);
	assert!(
		message.contains("`nope` is not a LaTeX environment"),
		"{message}"
	);
}

#[test]
fn static_files() {
	let workspace = Workspace::new(
		"static-files",
		&[
			("nml.toml", "[html]\nstatic = \"static\"\n"),
			("index.nml", "A"),
			("static/style.css", "body {}"),
			("static/fonts/a.woff", "font"),
			("static/.well-known/x", "x"),
			// Copied as it is, not parsed.
			("static/page.nml", "@@@"),
		],
	);
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	assert_eq!(report.parsed, ["index.nml"]);
	assert_eq!(workspace.output("style.css"), "body {}");
	assert_eq!(workspace.output("fonts/a.woff"), "font");
	assert_eq!(workspace.output(".well-known/x"), "x");
	assert_eq!(workspace.output("page.nml"), "@@@");

	// Changed files are copied again, removed ones deleted with the directories they
	// leave empty. Other files of the output are left alone.
	workspace.write("static/style.css", "p {}");
	workspace.remove("static/fonts/a.woff");
	workspace.write("public/mine.txt", "mine");
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	assert!(report.written.is_empty());
	assert_eq!(workspace.output("style.css"), "p {}");
	assert!(!workspace.0.join("public/fonts").exists());
	assert_eq!(workspace.output("mine.txt"), "mine");

	// A copy edited in the output is restored.
	workspace.write("public/style.css", "edited");
	workspace.build();
	assert_eq!(workspace.output("style.css"), "p {}");

	// Files where the build writes its own are not copied.
	workspace.write("static/assets/a.png", "a");
	workspace.write("static/nml-code.css", "code");
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, [3014 @ 16..24, 3014 @ 16..24]);
	assert!(!workspace.0.join("public/assets").exists());
	assert_ne!(workspace.output("nml-code.css"), "code");
	workspace.remove("static/assets/a.png");
	workspace.remove("static/nml-code.css");

	// A page written where a static file is copied is not written.
	workspace.write("static/index.html", "static");
	let report = workspace.build();
	assert_eq!(
		report.diagnostics[0].kind,
		DiagnosticKind::OutputConflict {
			output: "index.html".into(),
			other: "static/index.html".into(),
		}
	);
	assert_diagnostics!(report.diagnostics, [3013]);
	assert_eq!(workspace.output("index.html"), "static");
	// Once the file is gone, the page is written in its place.
	workspace.remove("static/index.html");
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, []);
	assert_eq!(report.written, ["index.nml"]);
	assert!(body(&workspace.output("index.html")).contains('A'));

	// Without the static directory.
	fs::remove_dir_all(workspace.0.join("static")).unwrap();
	let report = workspace.build();
	assert_diagnostics!(report.diagnostics, [3015 @ 16..24]);
}

#[test]
fn every_rule_is_documented() {
	let language = crate::language();
	let mut undocumented = Vec::new();
	for (_, rule) in language.rules().iter() {
		if rule.doc.trim().is_empty() {
			undocumented.push(rule.name.clone());
		}
		// A snippet writes what its rule matches, from its opener.
		if let Some(opener) = rule.opener().as_ref().and_then(|o| o.literal()) {
			let opener = opener.trim_end();
			assert!(
				rule.snippet.is_empty() || rule.snippet.replace("\\$", "$").starts_with(opener),
				"{}: {:?} does not start with {opener:?}",
				rule.name,
				rule.snippet
			);
		}
	}
	assert_eq!(undocumented, Vec::<String>::new());
}

#[test]
fn builds_one_file_without_a_workspace() {
	let workspace = Workspace::new(
		"single",
		&[
			(
				"doc.nml",
				"# Title\n:include part\n![Cat](img/cat.png)\n\nSee [[here]] and :code[rs]`fn x()`.\n",
			),
			("part.nml", ":anchor here Here\n"),
			("img/cat.png", "cat"),
			// Not part of the build, and not parsed.
			("broken.nml", "**unclosed"),
			// Another build's asset in the same output directory.
			("assets/other.png", "other"),
		],
	);
	let file = workspace.0.join("doc.nml");
	let report = build_file(&file, None).unwrap();
	assert_diagnostics!(report.diagnostics, []);
	assert_eq!(report.parsed, ["doc.nml"]);
	assert_eq!(report.written, ["doc.nml"]);
	let page = fs::read_to_string(workspace.0.join("doc.html")).unwrap();
	assert!(page.contains("<title>Title</title>"), "{page}");
	assert!(page.contains("<a href=\"#here\">Here</a>"), "{page}");
	assert!(page.contains("<img src=\"assets/"), "{page}");
	assert!(page.contains("href=\"nml-code.css\""), "{page}");
	assert!(workspace.0.join("nml-code.css").exists());
	assert!(workspace.0.join("assets/other.png").exists());
	// No database or configuration is made.
	assert!(!workspace.0.join("nml.toml").exists());
	assert!(!workspace.0.join(".nml").exists());

	// Elsewhere, under another name, with the assets beside it.
	let out = workspace.0.join("out/page.html");
	let report = build_file(&file, Some(&out)).unwrap();
	assert_diagnostics!(report.diagnostics, []);
	let page = fs::read_to_string(&out).unwrap();
	let src = page
		.split("<img src=\"")
		.nth(1)
		.unwrap()
		.split('"')
		.next()
		.unwrap();
	assert!(workspace.0.join("out").join(src).exists(), "{src}");
	assert!(!workspace.0.join("out/doc.html").exists());

	// Without code, no code stylesheet.
	workspace.write("plain/p.nml", "Plain.\n");
	build_file(&workspace.0.join("plain/p.nml"), None).unwrap();
	assert!(workspace.0.join("plain/p.html").exists());
	assert!(!workspace.0.join("plain/nml-code.css").exists());

	// Errors are reported, and nothing is written.
	let report = build_file(&workspace.0.join("broken.nml"), None).unwrap();
	assert_eq!(report.failed, ["broken.nml"]);
	assert!(!workspace.0.join("broken.html").exists());
}
