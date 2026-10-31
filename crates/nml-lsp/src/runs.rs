//! `:run` blocks in the editor: a code lens with each block's status, commands that run
//! them on worker threads, and their result on hover. Blocks only run on request.

use std::time::Duration;
use std::time::Instant;

use lsp_types::CodeLens;
use lsp_types::CodeLensParams;
use lsp_types::Command;
use nml_lang::RunBlock;
use nml_render::RunStatus;
use nml_source::Url;

use crate::Result;
use crate::Server;
use crate::convert;

/// Runs one block: `[uri, line]`, the line of its `:run`.
pub(crate) const RUN_COMMAND: &str = "nml.run";

/// Runs the blocks of a document that have no completed result: `[uri]`.
pub(crate) const RUN_ALL_COMMAND: &str = "nml.runAll";

/// Everything a block's run printed, and its status: `[uri, line]`. For clients that show
/// it themselves.
pub(crate) const RUN_OUTPUT_COMMAND: &str = "nml.runOutput";

/// Asks the client to open the files of what a block's run printed: `[uri, line]`.
pub(crate) const SHOW_RUN_OUTPUT_COMMAND: &str = "nml.showRunOutput";

/// How many lines of a run's output and errors a hover shows, from their end.
const HOVER_LINES: usize = 20;

/// A run that ended on a worker thread: its key.
pub(crate) struct RunDone(pub String);

/// How long something took, for people.
fn duration(duration: Duration) -> String {
	if duration < Duration::from_secs(1) {
		format!("{} ms", duration.as_millis())
	} else {
		format!("{:.1} s", duration.as_secs_f64())
	}
}

/// Whether a block has a result that completed.
fn completed(block: &RunBlock) -> bool {
	(block.result.as_ref()).is_some_and(|result| result.status == RunStatus::Completed)
}

/// The blocks of `runs` written in the file at `url`.
fn in_file<'a>(runs: &'a [RunBlock], url: &'a Url) -> impl Iterator<Item = &'a RunBlock> {
	runs.iter()
		.filter(move |run| run.command.root().source().location() == url)
}

/// The block of `runs` whose `:run` is on `line` of the file at `url`.
fn block_on_line<'a>(runs: &'a [RunBlock], url: &'a Url, line: u64) -> Result<&'a RunBlock> {
	in_file(runs, url)
		.find(|block| u64::from(convert::range(&block.command).start.line) == line)
		.ok_or_else(|| format!("no `:run` block starts on line {}", line + 1).into())
}

impl Server {
	/// The block a command names with `[uri, line]`.
	fn block_argument(&self, command: &str, arguments: &[serde_json::Value]) -> Result<RunBlock> {
		let usage = || format!("`{command}` takes a document URI and the line of a `:run` block");
		let (Some(url), Some(line)) = (
			arguments.first().and_then(serde_json::Value::as_str),
			arguments.get(1).and_then(serde_json::Value::as_u64),
		) else {
			return Err(usage().into());
		};
		let url = Url::parse(url)?;
		let Some(analysis) = self.analyses.get(&url) else {
			return Err(format!("`{url}` is not open").into());
		};
		Ok(block_on_line(&analysis.runs, &url, line)?.clone())
	}

	/// `nml.runOutput`: the block's status, what it printed, and the files that hold it.
	pub(crate) fn run_output(&self, arguments: &[serde_json::Value]) -> Result<serde_json::Value> {
		let block = self.block_argument(RUN_OUTPUT_COMMAND, arguments)?;
		let running = self.running.contains_key(&block.key);
		let status = match (&block.result, running) {
			(_, true) => "running",
			(None, false) => "not run",
			(Some(result), false) => result.status.name(),
		};
		let mut output = serde_json::json!({
			"runner": block.runner,
			"name": block.name.as_ref().map(|name| name.content()),
			"status": status,
		});
		if let (Some(result), false) = (&block.result, running) {
			let (stdout, stderr) = self.log_files(&block)?;
			let extra = serde_json::json!({
				"duration_ms": result.duration.as_millis() as u64,
				"message": result.message,
				"files": result.files,
				"stdout": result.stdout,
				"stderr": result.stderr,
				"stdout_uri": stdout.map(|url| url.to_string()),
				"stderr_uri": stderr.map(|url| url.to_string()),
			});
			output
				.as_object_mut()
				.unwrap()
				.extend(extra.as_object().unwrap().clone());
		}
		Ok(output)
	}

	/// The files of what a block's run printed on each stream, written from its result if
	/// they are missing. `None` without a workspace.
	fn log_files(&self, block: &RunBlock) -> Result<(Option<Url>, Option<Url>)> {
		let (Some(root), Some(config), Some(result)) = (&self.root, &self.config, &block.result)
		else {
			return Ok((None, None));
		};
		let cache = nml_build::run_cache(root, config);
		let mut urls = Vec::new();
		for (stream, text) in [
			(nml_workspace::Stream::Stdout, &result.stdout),
			(nml_workspace::Stream::Stderr, &result.stderr),
		] {
			let path = cache.log(&block.key, stream);
			// Results of older versions have none.
			if !path.exists() {
				std::fs::write(&path, text)?;
			}
			urls.push(Url::from_file_path(&path).ok());
		}
		let stderr = urls.pop().flatten();
		Ok((urls.pop().flatten(), stderr))
	}

	/// `nml.showRunOutput`: asks the client to open what the block's run printed.
	pub(crate) fn show_run_output(&mut self, arguments: &[serde_json::Value]) -> Result<()> {
		let block = self.block_argument(SHOW_RUN_OUTPUT_COMMAND, arguments)?;
		let Some(result) = &block.result else {
			return Err("the block has not run".into());
		};
		if !self.show_document {
			return Err(format!(
				"the editor cannot open documents: `{RUN_OUTPUT_COMMAND}` gives the output"
			)
			.into());
		}
		let (stdout, stderr) = self.log_files(&block)?;
		let shown: Vec<_> = [(stdout, &result.stdout), (stderr, &result.stderr)]
			.into_iter()
			.filter_map(|(url, text)| url.filter(|_| !text.is_empty()))
			.collect();
		if shown.is_empty() {
			self.show_message(
				lsp_types::MessageType::INFO,
				"the run printed nothing".into(),
			);
		}
		for url in shown {
			let params = lsp_types::ShowDocumentParams {
				uri: std::str::FromStr::from_str(url.as_str())?,
				external: Some(false),
				take_focus: Some(true),
				selection: None,
			};
			self.send_request("window/showDocument", serde_json::to_value(params)?);
		}
		Ok(())
	}

	/// Sends a request to the client; its answer is ignored.
	pub(crate) fn send_request(&mut self, method: &str, params: serde_json::Value) {
		self.requests += 1;
		let id = lsp_server::RequestId::from(format!("nml-{}", self.requests));
		let request = lsp_server::Request::new(id, method.into(), params);
		let _ = self.connection.sender.send(request.into());
	}

	/// The status of each `:run` block of a document, and the commands to run them.
	pub(crate) fn code_lenses(&self, params: &CodeLensParams) -> Result<Vec<CodeLens>> {
		let url = Url::parse(params.text_document.uri.as_str())?;
		let Some(analysis) = self.analyses.get(&url) else {
			return Ok(Vec::new());
		};
		let blocks: Vec<_> = in_file(&analysis.runs, &url).collect();
		let mut lenses = Vec::new();
		let uri = serde_json::Value::String(url.to_string());
		let idle = |block: &&&RunBlock| !self.running.contains_key(&block.key);
		let not_run = (blocks.iter().filter(idle))
			.filter(|block| block.result.is_none())
			.count();
		let failed = (blocks.iter().filter(idle))
			.filter(|block| block.result.is_some() && !completed(block))
			.count();
		let mut pending = Vec::new();
		if not_run > 0 {
			pending.push(format!("{not_run} not run"));
		}
		if failed > 0 {
			pending.push(format!("{failed} failed"));
		}
		for (index, block) in blocks.iter().enumerate() {
			let range = convert::range(&block.command);
			let line = serde_json::Value::from(range.start.line);
			let lens = |title: String, command: &str, arguments: Vec<serde_json::Value>| CodeLens {
				range,
				command: Some(Command {
					title,
					command: command.into(),
					arguments: (!arguments.is_empty()).then_some(arguments),
				}),
				data: None,
			};
			// One lens a block, its status and its command: a lens without a command
			// cannot be clicked in every editor.
			let title = match (&block.result, self.running.contains_key(&block.key)) {
				(_, true) => "⏳ Running…".to_string(),
				(None, false) => "▶ Run".to_string(),
				(Some(result), false) => {
					let status = match result.status {
						RunStatus::Completed => "✓ Completed in",
						RunStatus::Failed => "✗ Failed after",
					};
					format!("{status} {} · ▶ Run again", duration(result.duration))
				}
			};
			lenses.push(lens(title, RUN_COMMAND, vec![uri.clone(), line.clone()]));
			let printed = (block.result.as_ref())
				.is_some_and(|result| !result.stdout.is_empty() || !result.stderr.is_empty());
			if printed && !self.running.contains_key(&block.key) {
				let output = vec![uri.clone(), line];
				lenses.push(lens("≡ Output".into(), SHOW_RUN_OUTPUT_COMMAND, output));
			}
			if index == 0 && blocks.len() > 1 && !pending.is_empty() {
				let title = format!("▶ Run all ({})", pending.join(", "));
				lenses.push(lens(title, RUN_ALL_COMMAND, vec![uri.clone()]));
			}
		}
		Ok(lenses)
	}

	/// `nml.run` and `nml.runAll`: starts runs on worker threads. They end with
	/// [`Server::run_done`].
	pub(crate) fn run_command(
		&mut self,
		command: &str,
		arguments: &[serde_json::Value],
	) -> Result<()> {
		let usage = || {
			format!(
				"`{command}` takes a document URI{}",
				match command {
					RUN_COMMAND => " and the line of a `:run` block",
					_ => "",
				}
			)
		};
		let url = arguments
			.first()
			.and_then(serde_json::Value::as_str)
			.ok_or_else(usage)?;
		let url = Url::parse(url)?;
		let Some(analysis) = self.analyses.get(&url) else {
			return Err(format!("`{url}` is not open").into());
		};
		let blocks: Vec<RunBlock> = match command {
			RUN_COMMAND => {
				let line = arguments
					.get(1)
					.and_then(serde_json::Value::as_u64)
					.ok_or_else(usage)?;
				vec![block_on_line(&analysis.runs, &url, line)?.clone()]
			}
			_ => in_file(&analysis.runs, &url)
				.filter(|block| !completed(block))
				.cloned()
				.collect(),
		};
		for block in &blocks {
			self.start_run(block)?;
		}
		self.refresh_code_lenses();
		Ok(())
	}

	/// Runs a block on a worker thread, unless it is running already.
	fn start_run(&mut self, block: &RunBlock) -> Result<()> {
		if self.running.contains_key(&block.key) {
			return Ok(());
		}
		let (Some(root), Some(config)) = (&self.root, &self.config) else {
			return Err("runs need a workspace with an `nml.toml`".into());
		};
		let runner = (config.runners.get(&block.runner))
			.ok_or_else(|| format!("`{}` is not a runner of nml.toml", block.runner))?
			.clone();
		let cache = nml_build::run_cache(root, config);
		let root = root.clone();
		let cwd = match block.file.rsplit_once('/') {
			Some((dir, _)) => root.join(dir),
			None => root.clone(),
		};
		let code = block.code.clone();
		let data = block.data.clone();
		let key = block.key.clone();
		let sender = self.run_sender.clone();
		self.running.insert(key.clone(), Instant::now());
		std::thread::Builder::new()
			.name("nml-run".into())
			.spawn(move || {
				cache.run(&key, |out| runner.run(&code, &data, &cwd, &root, out));
				let _ = sender.send(RunDone(key));
			})?;
		Ok(())
	}

	/// A run ended: the documents that have it are diagnosed again, with its result.
	pub(crate) fn run_done(&mut self, done: RunDone) -> Result<()> {
		self.running.remove(&done.0);
		let urls: Vec<Url> = (self.analyses.iter())
			.filter(|(_, analysis)| analysis.runs.iter().any(|run| run.key == done.0))
			.map(|(url, _)| url.clone())
			.collect();
		for url in urls {
			self.diagnose(&url)?;
		}
		self.refresh_code_lenses();
		Ok(())
	}

	/// Asks the client for the code lenses again, if it can.
	fn refresh_code_lenses(&mut self) {
		if !self.code_lens_refresh {
			return;
		}
		self.send_request("workspace/codeLens/refresh", serde_json::Value::Null);
	}

	/// The status of the `:run` block whose first line is at `offset`, in Markdown.
	pub(crate) fn run_hover(
		&self,
		url: &Url,
		offset: usize,
	) -> Option<(String, nml_source::Token)> {
		let analysis = self.analyses.get(url)?;
		let block = in_file(&analysis.runs, url).find(|block| {
			let start = block.command.start();
			let first_line = block.token.content().find('\n').unwrap_or(0);
			start <= offset && offset <= start + first_line
		})?;
		let name = match &block.name {
			Some(name) => format!(" `#{}`", name.content()),
			None => String::new(),
		};
		let mut value = format!("**Run**{name} with `{}`: ", block.runner);
		match (&block.result, self.running.contains_key(&block.key)) {
			(_, true) => value.push_str("running…"),
			(None, false) => value.push_str("not run yet"),
			(Some(result), false) => {
				match result.status {
					RunStatus::Completed => {
						value.push_str(&format!("completed in {}", duration(result.duration)))
					}
					RunStatus::Failed => {
						value.push_str(&format!("failed after {}", duration(result.duration)))
					}
				}
				if !result.files.is_empty() {
					let files: Vec<_> = (result.files.iter())
						.map(|file| format!("`{file}`"))
						.collect();
					value.push_str(&format!("\n\nFiles: {}", files.join(", ")));
				}
				for (title, text) in [("Output", &result.stdout), ("Errors", &result.stderr)] {
					let text = text.trim_end();
					if text.is_empty() {
						continue;
					}
					let lines: Vec<_> = text.lines().collect();
					let shown = &lines[lines.len().saturating_sub(HOVER_LINES)..];
					let more = match lines.len() > shown.len() {
						true => format!(" (last {} lines)", shown.len()),
						false => String::new(),
					};
					value.push_str(&format!(
						"\n\n{title}{more}:\n```text\n{}\n```",
						shown.join("\n")
					));
				}
			}
		}
		Some((value, block.command.clone()))
	}
}
