//! A live preview of a workspace's pages in the browser.
//!
//! An HTTP server on the loopback interface serves the pages from memory, through
//! [`nml_build::Preview`], with a script that listens to the server's events
//! (Server-Sent Events): the page's new content as the editor changes it, and where the
//! editor's cursor is. Only the pages a browser shows are rendered.

use std::collections::HashMap;
use std::collections::HashSet;
use std::io;
use std::io::Read;
use std::io::Write;
use std::net::SocketAddr;
use std::net::TcpListener;
use std::net::TcpStream;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use crossbeam_channel::Receiver;
use crossbeam_channel::RecvTimeoutError;
use crossbeam_channel::Sender;
use nml_build::Preview;
use nml_build::Served;

/// The script pages are served with.
const SCRIPT: &str = include_str!("preview.js");
const SCRIPT_URL: &str = "/__nml/preview.js";
const EVENTS_URL: &str = "/__nml/events";

/// How long edits settle before the pages are rendered again…
const DEBOUNCE: Duration = Duration::from_millis(250);
/// …unless edits keep coming for this long.
const MAX_WAIT: Duration = Duration::from_secs(1);
/// How often idle event streams are written to, to notice the browsers that left.
const HEARTBEAT: Duration = Duration::from_secs(15);
/// The largest request head read.
const MAX_REQUEST: usize = 16 * 1024;

/// A running preview server. It stops when dropped.
#[derive(Debug)]
pub struct PreviewServer {
	address: SocketAddr,
	sender: Sender<Message>,
	stopped: Arc<AtomicBool>,
}

/// What the worker, which owns the workspace, is asked.
enum Message {
	/// The response to a request of `url`, already percent-decoded.
	Get {
		url: String,
		reply: Sender<HttpResponse>,
	},
	/// A browser listens to the events of the page at a workspace path.
	Subscribe {
		page: String,
		stream: TcpStream,
	},
	/// The unsaved contents of the editor's files, by workspace path.
	Edited(HashMap<String, String>),
	/// Files were saved, and the index updated.
	Saved,
	/// The editor's cursor is at `offset` in the source of the page at a workspace path.
	Cursor {
		page: String,
		offset: usize,
	},
	/// The URL of the page at a workspace path.
	PageUrl {
		path: String,
		reply: Sender<Option<String>>,
	},
	Stop,
}

struct HttpResponse {
	status: u16,
	content_type: &'static str,
	body: Vec<u8>,
}

impl HttpResponse {
	fn text(status: u16, text: &str) -> Self {
		Self {
			status,
			content_type: "text/plain; charset=utf-8",
			body: text.as_bytes().to_vec(),
		}
	}
}

impl PreviewServer {
	/// Starts serving the workspace at `root` on a free port of `127.0.0.1`.
	pub fn start(root: &Path) -> io::Result<Self> {
		Self::start_on(root, 0)
	}

	/// Starts serving the workspace at `root` on `port` of `127.0.0.1`, or on a free one if
	/// it is 0.
	pub fn start_on(root: &Path, port: u16) -> io::Result<Self> {
		let listener = TcpListener::bind(("127.0.0.1", port))?;
		let address = listener.local_addr()?;
		let (sender, receiver) = crossbeam_channel::unbounded();
		// The workspace is opened by the worker, which keeps it: it is not shared.
		let (ready, opened) = crossbeam_channel::bounded(1);
		let root: PathBuf = root.into();
		let port = address.port();
		thread::Builder::new()
			.name("nml-preview".into())
			.spawn(move || match Preview::open(&root) {
				Ok(preview) => {
					let _ = ready.send(Ok(()));
					Worker::new(preview, port).run(&receiver);
				}
				Err(error) => {
					let _ = ready.send(Err(error.to_string()));
				}
			})?;
		opened
			.recv()
			.map_err(io::Error::other)?
			.map_err(io::Error::other)?;

		let stopped = Arc::new(AtomicBool::new(false));
		let accepting = sender.clone();
		let stop = stopped.clone();
		thread::Builder::new()
			.name("nml-preview-accept".into())
			.spawn(move || {
				for stream in listener.incoming() {
					if stop.load(Ordering::Relaxed) {
						break;
					}
					let Ok(stream) = stream else { continue };
					let sender = accepting.clone();
					let _ = thread::Builder::new()
						.name("nml-preview-request".into())
						.spawn(move || {
							let _ = handle(stream, port, &sender);
						});
				}
			})?;
		Ok(Self {
			address,
			sender,
			stopped,
		})
	}

	pub fn address(&self) -> SocketAddr {
		self.address
	}

	/// The URL of the page at the workspace path `path`, if it is a page the index knows.
	pub fn page_url(&self, path: &str) -> Option<String> {
		let (reply, answer) = crossbeam_channel::bounded(1);
		let path = path.to_string();
		self.sender.send(Message::PageUrl { path, reply }).ok()?;
		answer.recv().ok()?
	}

	/// The unsaved contents of the editor's files, by workspace path: the pages shown are
	/// rendered again once edits settle.
	pub fn edited(&self, overlay: HashMap<String, String>) {
		let _ = self.sender.send(Message::Edited(overlay));
	}

	/// Files were saved and the index updated: the configuration is read again, and the
	/// pages shown are rendered again, with the renders that failed retried.
	pub fn saved(&self) {
		let _ = self.sender.send(Message::Saved);
	}

	/// The editor's cursor is at the byte `offset` of the page at the workspace path `page`:
	/// the browsers that show it scroll there.
	pub fn cursor(&self, page: &str, offset: usize) {
		let page = page.to_string();
		let _ = self.sender.send(Message::Cursor { page, offset });
	}
}

impl Drop for PreviewServer {
	fn drop(&mut self) {
		self.stopped.store(true, Ordering::Relaxed);
		let _ = self.sender.send(Message::Stop);
		// Wakes the accepting thread, which then sees it is stopped.
		let _ = TcpStream::connect_timeout(&self.address, Duration::from_secs(1));
	}
}

/// Answers one HTTP request.
fn handle(mut stream: TcpStream, port: u16, sender: &Sender<Message>) -> io::Result<()> {
	stream.set_read_timeout(Some(Duration::from_secs(5)))?;
	let mut head = Vec::new();
	let mut buffer = [0; 2048];
	while !head.windows(4).any(|w| w == b"\r\n\r\n") {
		let read = stream.read(&mut buffer)?;
		if read == 0 || head.len() > MAX_REQUEST {
			return Ok(());
		}
		head.extend_from_slice(&buffer[..read]);
	}
	let head = String::from_utf8_lossy(&head);
	let mut lines = head.split("\r\n");
	let request = lines.next().unwrap_or_default();
	let mut parts = request.split(' ');
	let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
	let host = lines
		.filter_map(|line| line.split_once(':'))
		.find(|(name, _)| name.eq_ignore_ascii_case("host"))
		.map(|(_, value)| value.trim());

	let response = if method != "GET" && method != "HEAD" {
		HttpResponse::text(405, "Only GET")
	// Pages of another site that resolve their name to this address (DNS rebinding) do not
	// read the workspace.
	} else if !host.is_some_and(|host| is_local(host, port)) {
		HttpResponse::text(403, "Not a local request")
	} else {
		let (path, query) = target.split_once('?').unwrap_or((target, ""));
		let path = percent_decode(path);
		if path == SCRIPT_URL {
			HttpResponse {
				status: 200,
				content_type: "text/javascript; charset=utf-8",
				body: SCRIPT.as_bytes().to_vec(),
			}
		} else if path == EVENTS_URL {
			let page = query
				.split('&')
				.find_map(|pair| pair.strip_prefix("page="))
				.map(percent_decode);
			match page {
				Some(page) => {
					let _ = sender.send(Message::Subscribe { page, stream });
					return Ok(());
				}
				None => HttpResponse::text(400, "Which page?"),
			}
		} else {
			let (reply, answer) = crossbeam_channel::bounded(1);
			let _ = sender.send(Message::Get { url: path, reply });
			match answer.recv() {
				Ok(response) => response,
				Err(_) => HttpResponse::text(503, "The preview stopped"),
			}
		}
	};
	let reason = match response.status {
		200 => "OK",
		400 => "Bad Request",
		403 => "Forbidden",
		404 => "Not Found",
		405 => "Method Not Allowed",
		500 => "Internal Server Error",
		_ => "Service Unavailable",
	};
	write!(
		stream,
		"HTTP/1.1 {} {reason}\r\nContent-Type: {}\r\nContent-Length: {}\r\n\
		Cache-Control: no-store\r\nConnection: close\r\n\r\n",
		response.status,
		response.content_type,
		response.body.len()
	)?;
	if method != "HEAD" {
		stream.write_all(&response.body)?;
	}
	stream.flush()
}

/// Whether a `Host` header names this server.
fn is_local(host: &str, port: u16) -> bool {
	host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
}

/// A browser that listens to the events of a page.
struct Client {
	/// The page's workspace path.
	page: String,
	stream: TcpStream,
	/// The page as the browser has it.
	html: String,
}

/// Owns the workspace: renders pages and sends their events.
struct Worker {
	preview: Preview,
	port: u16,
	clients: Vec<Client>,
	/// The last page served for each workspace path: what a browser that subscribes has.
	served: HashMap<String, String>,
	/// When the pages shown are rendered again, and since when they are out of date.
	deadline: Option<Instant>,
	dirty_since: Option<Instant>,
	/// Renders that failed, by key: their source shows, and the editor says why.
	failed: HashSet<String>,
}

impl Worker {
	fn new(preview: Preview, port: u16) -> Self {
		Self {
			preview,
			port,
			clients: Vec::new(),
			served: HashMap::new(),
			deadline: None,
			dirty_since: None,
			failed: HashSet::new(),
		}
	}

	fn run(mut self, receiver: &Receiver<Message>) {
		loop {
			let timeout = match self.deadline {
				Some(deadline) => deadline.saturating_duration_since(Instant::now()),
				None => HEARTBEAT,
			};
			match receiver.recv_timeout(timeout) {
				Ok(Message::Get { url, reply }) => {
					let _ = reply.send(self.get(&url));
				}
				Ok(Message::Subscribe { page, stream }) => self.subscribe(page, stream),
				Ok(Message::Edited(overlay)) => {
					self.preview.set_overlay(overlay);
					self.schedule(DEBOUNCE);
				}
				Ok(Message::Saved) => {
					let _ = self.preview.reload_config();
					// What a renderer could not do may work now: it may have been installed,
					// or its configuration changed.
					self.failed.clear();
					self.schedule(Duration::ZERO);
				}
				Ok(Message::Cursor { page, offset }) => {
					let data = offset.to_string();
					self.send(|client| client.page == page, "scroll", &data);
				}
				Ok(Message::PageUrl { path, reply }) => {
					let output = self.preview.output_of(&path).ok().flatten();
					let url = output.map(|output| {
						format!("http://127.0.0.1:{}/{}", self.port, percent_encode(&output))
					});
					let _ = reply.send(url);
				}
				Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
				Err(RecvTimeoutError::Timeout) => match self.deadline {
					Some(_) => self.refresh(),
					None => self.send(|_| true, "", ""),
				},
			}
		}
	}

	/// Renders the pages shown again, in `delay`, or at most [`MAX_WAIT`] after the first
	/// change that is not shown yet.
	fn schedule(&mut self, delay: Duration) {
		let now = Instant::now();
		let first = *self.dirty_since.get_or_insert(now);
		self.deadline = Some((now + delay).min(first + MAX_WAIT));
	}

	fn get(&mut self, url: &str) -> HttpResponse {
		let served = match self.preview.lookup(url) {
			Ok(Some(served)) => served,
			Ok(None) => return HttpResponse::text(404, "Not found"),
			Err(error) => return HttpResponse::text(500, &error.to_string()),
		};
		match served {
			Served::File { content_type, body } => HttpResponse {
				status: 200,
				content_type,
				body,
			},
			Served::Page(path) => match self.page(&path) {
				Some(html) => {
					self.served.insert(path, html.clone());
					HttpResponse {
						status: 200,
						content_type: "text/html; charset=utf-8",
						body: html.into_bytes(),
					}
				}
				None => HttpResponse::text(500, &format!("`{path}` cannot be shown")),
			},
		}
	}

	/// The page at a workspace path as it is now, with the preview's script.
	fn page(&mut self, path: &str) -> Option<String> {
		let page = self.preview.page(path).ok()??;
		// Rendered once a browser listens.
		if page.pending.iter().any(|p| !self.failed.contains(&p.key)) {
			self.schedule(Duration::ZERO);
		}
		Some(with_script(&page.html, path))
	}

	fn subscribe(&mut self, page: String, mut stream: TcpStream) {
		let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
			Cache-Control: no-store\r\nConnection: keep-alive\r\n\r\nretry: 1000\n\n";
		if stream.write_all(head.as_bytes()).is_err() {
			return;
		}
		let html = self.served.get(&page).cloned().unwrap_or_default();
		self.clients.push(Client { page, stream, html });
		// It may have changed since it was served.
		self.schedule(Duration::ZERO);
	}

	/// Renders the pages shown, and sends what changed. Renders not in the cache are made
	/// then, and the pages rendered again.
	fn refresh(&mut self) {
		self.deadline = None;
		self.dirty_since = None;
		let mut pages: Vec<String> = self.clients.iter().map(|c| c.page.clone()).collect();
		pages.sort();
		pages.dedup();
		let mut pending = Vec::new();
		for path in pages {
			let Ok(Some(page)) = self.preview.page(&path) else {
				continue;
			};
			let html = with_script(&page.html, &path);
			self.served.insert(path.clone(), html.clone());
			let mut gone = Vec::new();
			for (index, client) in self.clients.iter_mut().enumerate() {
				if client.page == path && update(client, &html).is_err() {
					gone.push(index);
				}
			}
			for index in gone.into_iter().rev() {
				self.clients.remove(index);
			}
			pending.extend(page.pending);
		}
		pending.retain(|p| !self.failed.contains(&p.key));
		pending.sort_by(|a, b| a.key.cmp(&b.key));
		pending.dedup();
		let mut rendered = false;
		for render in pending {
			match self.preview.render(&render) {
				Ok(Ok(())) => rendered = true,
				_ => {
					self.failed.insert(render.key);
				}
			}
		}
		if rendered {
			self.schedule(Duration::ZERO);
		}
	}

	/// Sends an event to the clients `to` selects; an empty name sends a comment, which
	/// browsers ignore. Clients that left are dropped.
	fn send(&mut self, to: impl Fn(&Client) -> bool, name: &str, data: &str) {
		self.clients.retain_mut(|client| {
			!to(client) || write_event(&mut client.stream, name, data).is_ok()
		});
	}
}

/// Sends a client what changed in its page: its content, or everything around it.
fn update(client: &mut Client, html: &str) -> io::Result<()> {
	if client.html == html {
		return Ok(());
	}
	let (old_main, old_around) = split(&client.html);
	let (main, around) = split(html);
	if old_main.is_none() || main.is_none() || old_around != around {
		write_event(&mut client.stream, "reload", "")?;
	} else if old_main != main {
		write_event(&mut client.stream, "main", main.unwrap_or_default())?;
	}
	client.html = html.to_string();
	Ok(())
}

const MAIN_START: &str = "<main class=\"nml-main\">\n";
const MAIN_END: &str = "</main>";

/// The content of a page's `<main>`, and the page around it.
fn split(html: &str) -> (Option<&str>, (&str, &str)) {
	let start = html.find(MAIN_START).map(|start| start + MAIN_START.len());
	let end = html.rfind(MAIN_END);
	match (start, end) {
		(Some(start), Some(end)) if start <= end => {
			(Some(&html[start..end]), (&html[..start], &html[end..]))
		}
		_ => (None, (html, "")),
	}
}

/// Writes a server-sent event; without a name, a comment.
fn write_event(stream: &mut TcpStream, name: &str, data: &str) -> io::Result<()> {
	let mut event = String::new();
	if name.is_empty() {
		event.push_str(":\n");
	} else {
		event.push_str("event: ");
		event.push_str(name);
		event.push('\n');
		for line in data.split('\n') {
			event.push_str("data: ");
			event.push_str(line.strip_suffix('\r').unwrap_or(line));
			event.push('\n');
		}
	}
	event.push('\n');
	stream.write_all(event.as_bytes())?;
	stream.flush()
}

/// A page with the preview's script, which listens to the events of `page`.
fn with_script(html: &str, page: &str) -> String {
	let mut script = format!("<script src=\"{SCRIPT_URL}\" data-page=\"");
	for c in page.chars() {
		match c {
			'&' => script.push_str("&amp;"),
			'"' => script.push_str("&quot;"),
			'<' => script.push_str("&lt;"),
			c => script.push(c),
		}
	}
	script.push_str("\" defer></script>\n");
	match html.find("</head>") {
		Some(at) => format!("{}{script}{}", &html[..at], &html[at..]),
		None => format!("{script}{html}"),
	}
}

/// Decodes `%XX` escapes; invalid ones stay as they are.
fn percent_decode(text: &str) -> String {
	let bytes = text.as_bytes();
	let mut out = Vec::with_capacity(bytes.len());
	let mut i = 0;
	while i < bytes.len() {
		let hex = |b: u8| (b as char).to_digit(16);
		if bytes[i] == b'%'
			&& let (Some(Some(high)), Some(Some(low))) = (
				bytes.get(i + 1).map(|b| hex(*b)),
				bytes.get(i + 2).map(|b| hex(*b)),
			) {
			out.push((high * 16 + low) as u8);
			i += 3;
			continue;
		}
		out.push(bytes[i]);
		i += 1;
	}
	String::from_utf8_lossy(&out).into_owned()
}

/// Escapes a path for a URL, but its `/`.
fn percent_encode(path: &str) -> String {
	let mut out = String::new();
	for byte in path.bytes() {
		if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
			out.push(byte as char);
		} else {
			out.push_str(&format!("%{byte:02X}"));
		}
	}
	out
}

#[cfg(test)]
mod tests;
