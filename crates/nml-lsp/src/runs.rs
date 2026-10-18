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

impl Server {
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
			let run = vec![uri.clone(), line];
			if self.running.contains_key(&block.key) {
				lenses.push(lens("⏳ Running…".into(), "", Vec::new()));
			} else {
				let (status, action) = match &block.result {
					None => (None, "▶ Run"),
					Some(result) if result.status == RunStatus::Completed => (
						Some(format!("✓ Completed in {}", duration(result.duration))),
						"▶ Run again",
					),
					Some(result) => (
						Some(format!("✗ Failed after {}", duration(result.duration))),
						"▶ Run again",
					),
				};
				if let Some(status) = status {
					lenses.push(lens(status, "", Vec::new()));
				}
				lenses.push(lens(action.into(), RUN_COMMAND, run));
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
				let block = in_file(&analysis.runs, &url)
					.find(|block| u64::from(convert::range(&block.command).start.line) == line)
					.ok_or_else(|| format!("no `:run` block starts on line {}", line + 1))?;
				vec![block.clone()]
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
		let key = block.key.clone();
		let sender = self.run_sender.clone();
		self.running.insert(key.clone(), Instant::now());
		std::thread::Builder::new()
			.name("nml-run".into())
			.spawn(move || {
				cache.run(&key, |out| runner.run(&code, &cwd, &root, out));
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
		self.requests += 1;
		let id = lsp_server::RequestId::from(format!("nml-{}", self.requests));
		let request = lsp_server::Request::new(
			id,
			"workspace/codeLens/refresh".into(),
			serde_json::Value::Null,
		);
		let _ = self.connection.sender.send(request.into());
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
