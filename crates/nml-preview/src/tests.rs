use std::fs;
use std::io::BufRead;
use std::io::BufReader;

use super::*;

/// Skips a test that needs a program which is not installed, unless `NML_REQUIRE_RENDERERS`
/// is set, as in CI, where it fails instead.
fn skip(missing: &str) {
	assert!(
		std::env::var_os("NML_REQUIRE_RENDERERS").is_none(),
		"no {missing}, but NML_REQUIRE_RENDERERS is set"
	);
	eprintln!("skipped: no {missing}");
}

/// A temporary workspace, built, removed when dropped.
struct Workspace(PathBuf);

impl Drop for Workspace {
	fn drop(&mut self) {
		let _ = fs::remove_dir_all(&self.0);
	}
}

impl Workspace {
	fn new(name: &str, files: &[(&str, &str)]) -> Self {
		let dir = std::env::temp_dir().join(format!("nml-preview-{name}-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		let workspace = Self(dir);
		for (path, content) in files {
			workspace.write(path, content);
		}
		workspace.build();
		workspace
	}

	fn write(&self, path: &str, content: &str) {
		let path = self.0.join(path);
		fs::create_dir_all(path.parent().unwrap()).unwrap();
		fs::write(path, content).unwrap();
	}

	/// Built as the language server does: without writing.
	fn build(&self) {
		let options = nml_build::BuildOptions { write: false };
		nml_build::build_with(&self.0, options).unwrap();
	}
}

/// A request to the server, with a `Host` header: its status and body.
fn request(server: &PreviewServer, method: &str, target: &str, host: &str) -> (u16, String) {
	let mut stream = TcpStream::connect(server.address()).unwrap();
	write!(stream, "{method} {target} HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
	let mut response = String::new();
	stream.read_to_string(&mut response).unwrap();
	let status = response[9..12].parse().unwrap();
	let body = response.split_once("\r\n\r\n").unwrap().1.to_string();
	(status, body)
}

fn get(server: &PreviewServer, target: &str) -> (u16, String) {
	let host = format!("127.0.0.1:{}", server.address().port());
	request(server, "GET", target, &host)
}

/// The events a browser receives.
struct Events(BufReader<TcpStream>);

impl Events {
	fn open(server: &PreviewServer, page: &str) -> Self {
		let mut stream = TcpStream::connect(server.address()).unwrap();
		stream
			.set_read_timeout(Some(Duration::from_secs(10)))
			.unwrap();
		let port = server.address().port();
		write!(
			stream,
			"GET {EVENTS_URL}?page={} HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n",
			percent_encode(page)
		)
		.unwrap();
		let mut events = Self(BufReader::new(stream));
		// The response's head.
		let mut line = String::new();
		while line != "\r\n" {
			line.clear();
			events.0.read_line(&mut line).unwrap();
			if line.starts_with("HTTP/1.1") {
				assert!(line.starts_with("HTTP/1.1 200"), "{line}");
			}
		}
		events
	}

	/// The next event, as its name and data.
	fn next(&mut self) -> (String, String) {
		let mut name = String::new();
		let mut data: Vec<String> = Vec::new();
		loop {
			let mut line = String::new();
			let read = self.0.read_line(&mut line).expect("an event in time");
			assert!(read > 0, "the stream ended");
			let line = line.trim_end_matches('\n');
			if line.is_empty() {
				if !name.is_empty() {
					return (name, data.join("\n"));
				}
				continue;
			}
			if let Some(value) = line.strip_prefix("event: ") {
				name = value.to_string();
			} else if let Some(value) = line.strip_prefix("data: ") {
				data.push(value.to_string());
			}
		}
	}

	/// The next event named `name`, skipping others.
	fn next_named(&mut self, name: &str) -> String {
		loop {
			let (event, data) = self.next();
			if event == name {
				return data;
			}
		}
	}
}

#[test]
fn serves_pages_with_the_script() {
	let workspace = Workspace::new(
		"serve",
		&[
			("nml.toml", ""),
			("index.nml", "# Home\n"),
			("docs/My guide.nml", "# Guide\n"),
		],
	);
	let server = PreviewServer::start(&workspace.0).unwrap();
	assert!(server.address().ip().is_loopback());

	let (status, page) = get(&server, "/");
	assert_eq!(status, 200);
	assert!(
		page.contains(
			"<script src=\"/__nml/preview.js\" data-page=\"index.nml\" defer></script>\n</head>"
		),
		"{page}"
	);
	assert!(page.contains("<h1 data-nml-at=\"0\""), "{page}");
	let (status, page) = get(&server, "/docs/My%20guide.html");
	assert_eq!(status, 200);
	assert!(page.contains("data-page=\"docs/My guide.nml\""), "{page}");
	assert_eq!(
		server.page_url("docs/My guide.nml").as_deref(),
		Some(
			format!(
				"http://127.0.0.1:{}/docs/My%20guide.html",
				server.address().port()
			)
			.as_str()
		)
	);
	assert_eq!(server.page_url("nope.nml"), None);

	let (status, script) = get(&server, SCRIPT_URL);
	assert_eq!(status, 200);
	assert_eq!(script, SCRIPT);
	let (status, css) = get(&server, "/nml.css");
	assert_eq!(status, 200);
	assert_eq!(css, nml_html_default_style());

	assert_eq!(get(&server, "/nope.html").0, 404);
	assert_eq!(get(&server, "/../nml.toml").0, 404);
	assert_eq!(get(&server, "/%2e%2e/nml.toml").0, 404);
	assert_eq!(get(&server, EVENTS_URL).0, 400);
	// Only local names, and only reads.
	let port = server.address().port();
	assert_eq!(
		request(&server, "GET", "/", &format!("evil.example:{port}")).0,
		403
	);
	assert_eq!(request(&server, "GET", "/", "127.0.0.1:1").0, 403);
	assert_eq!(
		request(&server, "POST", "/", &format!("127.0.0.1:{port}")).0,
		405
	);
	let (status, body) = request(&server, "HEAD", "/", &format!("localhost:{port}"));
	assert_eq!((status, body.as_str()), (200, ""));
}

/// The default stylesheet, as the build writes it.
fn nml_html_default_style() -> String {
	let workspace = Workspace::new("style", &[("nml.toml", ""), ("a.nml", "a")]);
	nml_build::build(&workspace.0).unwrap();
	fs::read_to_string(workspace.0.join("public/nml.css")).unwrap()
}

#[test]
fn pages_follow_the_editor() {
	let workspace = Workspace::new(
		"follow",
		&[
			("nml.toml", ""),
			("a.nml", "# A\nFirst.\n"),
			("b.nml", "# B\n"),
		],
	);
	let server = PreviewServer::start(&workspace.0).unwrap();
	let (_, page) = get(&server, "/a.html");
	assert!(page.contains("First."));
	let mut events = Events::open(&server, "a.nml");

	// Unsaved edits: the new content.
	server.edited(HashMap::from([(
		"a.nml".to_string(),
		"# A\nSecond.\n\nThird.\n".to_string(),
	)]));
	let main = events.next_named("main");
	assert!(main.contains("<p data-nml-at=\"4\">Second.</p>"), "{main}");
	assert!(main.contains("Third."), "{main}");
	assert!(!main.contains("<main"), "{main}");

	// The title is around the content: the page reloads.
	server.edited(HashMap::from([(
		"a.nml".to_string(),
		"# Retitled\nSecond.\n".to_string(),
	)]));
	assert_eq!(events.next_named("reload"), "");

	// The cursor, for the browsers of that page only.
	let mut other = Events::open(&server, "b.nml");
	server.cursor("b.nml", 2);
	assert_eq!(other.next_named("scroll"), "2");
	server.cursor("a.nml", 12);
	assert_eq!(events.next_named("scroll"), "12");

	// Another page saved and built: the reference shows its new title.
	server.edited(HashMap::from([(
		"a.nml".to_string(),
		"# Retitled\nSee [[b#]].\n".to_string(),
	)]));
	assert!(events.next_named("main").contains(">B</a>"));
	workspace.write("b.nml", "# Bee\n");
	workspace.build();
	server.saved();
	assert!(events.next_named("main").contains(">Bee</a>"));
}

#[test]
fn renders_arrive_later() {
	if std::process::Command::new("dot")
		.arg("-V")
		.output()
		.is_err()
	{
		skip("`dot`");
		return;
	}
	let workspace = Workspace::new("render", &[("nml.toml", ""), ("a.nml", "a")]);
	let server = PreviewServer::start(&workspace.0).unwrap();
	get(&server, "/a.html");
	let mut events = Events::open(&server, "a.nml");
	server.edited(HashMap::from([(
		"a.nml".to_string(),
		":graph\n```\ndigraph { preview -> svg }\n```\n".to_string(),
	)]));
	// The source first, then the picture.
	let main = events.next_named("main");
	assert!(main.contains("<code>digraph"), "{main}");
	let main = events.next_named("main");
	assert!(main.contains("<svg"), "{main}");
}

#[test]
fn a_workspace_is_needed() {
	let dir = std::env::temp_dir().join(format!("nml-preview-none-{}", std::process::id()));
	fs::create_dir_all(&dir).unwrap();
	assert!(PreviewServer::start(&dir).is_err());
	fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn helpers() {
	assert_eq!(percent_decode("a%20b%2Fc%zz%4"), "a b/c%zz%4");
	assert_eq!(
		percent_encode("docs/My guide é.html"),
		"docs/My%20guide%20%C3%A9.html"
	);
	assert_eq!(percent_decode(&percent_encode("a b/é")), "a b/é");
	assert!(is_local("127.0.0.1:80", 80));
	assert!(is_local("localhost:80", 80));
	assert!(!is_local("localhost", 80));
	assert!(!is_local("127.0.0.1.evil:80", 80));

	let html =
		"<head>\n<title>x</title>\n</head>\n<body>\n<main class=\"nml-main\">\nA\n</main>\n</body>";
	let (main, around) = split(html);
	assert_eq!(main, Some("A\n"));
	assert_eq!(around.1, "</main>\n</body>");
	assert_eq!(split("no main").0, None);
	assert_eq!(
		with_script("<head></head>", "a\"b.nml"),
		"<head><script src=\"/__nml/preview.js\" data-page=\"a&quot;b.nml\" defer></script>\n</head>"
	);
}

#[test]
fn the_configuration_is_read_again() {
	let workspace = Workspace::new(
		"config",
		&[
			("nml.toml", "[html]\nlanguage = \"en\"\n"),
			("a.nml", "# A\n"),
		],
	);
	let server = PreviewServer::start(&workspace.0).unwrap();
	let (_, page) = get(&server, "/a.html");
	assert!(page.contains("<html lang=\"en\">"), "{page}");
	let mut events = Events::open(&server, "a.nml");

	// Around the content: the page reloads, in the new language.
	workspace.write("nml.toml", "[html]\nlanguage = \"fr\"\n");
	workspace.build();
	server.saved();
	assert_eq!(events.next_named("reload"), "");
	let (_, page) = get(&server, "/a.html");
	assert!(page.contains("<html lang=\"fr\">"), "{page}");

	// An invalid one is not used.
	workspace.write("nml.toml", "pages = 1\n");
	server.saved();
	let (status, page) = get(&server, "/a.html");
	assert_eq!(status, 200);
	assert!(page.contains("<html lang=\"fr\">"), "{page}");
}
