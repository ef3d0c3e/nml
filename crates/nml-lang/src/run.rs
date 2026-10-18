//! `:run` blocks: code run by a runner of the configuration, whose files later elements
//! show with `run:name` URLs.

use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::Attributes;
use nml_document::Dependency;
use nml_document::ElementKind;
use nml_grammar::Atom;
use nml_grammar::OptionKind;
use nml_grammar::OptionSchema;
use nml_grammar::OptionSpec;
use nml_grammar::OptionValue;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::RuleMatch;
use nml_grammar::Value;
use nml_render::RunResult;
use nml_render::RunStatus;
use nml_source::Token;

use crate::Language;
use crate::Lower;
use crate::resolve::RunError;
use crate::resolve::RunFile;
use crate::resolve::RunRequest;

/// The prefix of URLs that name the files of a run: `run:name` or `run:name/file`.
pub const RUN_SCHEME: &str = "run:";

/// What a `:run` block shows.
const SHOW: &[&str] = &["none", "code", "output", "both"];

/// The class of the preformatted output of a run.
pub const OUTPUT_CLASS: &str = "run-output";

/// A `:run` block of a unit, for editors and the build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunBlock {
	/// Its name, after `#`.
	pub name: Option<Token>,
	/// Its runner, of `[run.<name>]`.
	pub runner: String,
	pub code: String,
	/// The workspace path of the file it is written in: it runs in that file's directory.
	pub file: String,
	/// The block, from `:run` to the closing fence.
	pub token: Token,
	/// `:run`.
	pub command: Token,
	/// The key of its result.
	pub key: String,
	/// Its result, if it ran.
	pub result: Option<RunResult>,
	/// The files it wrote, if it completed.
	pub files: Vec<RunFile>,
}

pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let schema = OptionSchema::new([
		OptionSpec::new(
			"show",
			OptionKind::Choice(SHOW.iter().map(|s| s.to_string()).collect()),
		)
		.with_default(OptionValue::Text("none".into()))
		.with_doc(
			"What the block shows: `none`, its `code`, its `output` (what it printed), or \
				`both`.",
		),
		OptionSpec::new("inputs", OptionKind::Text)
			.with_default(OptionValue::Text(String::new()))
			.with_doc(
				"The files the code reads, separated by spaces: it runs again when they \
				change.",
			),
	]);
	let rule = Rule::new(
		"run",
		vec![
			Atom::LineStart,
			Atom::capture("command", Atom::literal(":run")).with_class("keyword"),
			Atom::optional(vec![Atom::capture(
				"options",
				Atom::Options(Arc::new(schema)),
			)]),
			Atom::optional(vec![
				Atom::literal(" #"),
				Atom::capture("name", Atom::Word).with_class("variable"),
			]),
			Atom::LineEnd,
			Atom::LineStart,
			Atom::capture("fence", Atom::run('`', 3)).with_class("operator"),
			Atom::capture("info", Atom::RestOfLine).with_class("type"),
			Atom::LineEnd,
			Atom::capture("source", Atom::Text),
			Atom::LineStart,
			Atom::capture("close", Atom::same("fence")).with_class("operator"),
			Atom::LineEnd,
		],
	)
	.with_doc(
		"**Run** code with a runner of `nml.toml` (`[run.<name>]`) when the page is built.\n\n\
		````nml\n:run #plot\n```R\nsvg(file.path(Sys.getenv(\"NML_OUT\"), \"plot.svg\"))\n\
		plot(cars)\ninvisible(dev.off())\n```\n![Cars](run:plot)\n````\n\n\
		The fence names the runner. The files the code writes in `$NML_OUT` are shown with \
		`run:name/file`, or `run:name` for its only file. Options: `show` (`none`, `code`, \
		`output`, `both`) and `inputs`, the files it reads.",
	)
	.with_snippet(":run #${1:name}\n```${2:runner}\n${0}\n```");
	language.add(rule, lower_run)?;
	Ok(())
}

fn lower_run(lower: &mut Lower<'_, '_>, m: &RuleMatch) {
	let info = m.capture("info").unwrap().token.clone();
	let runner = info.content().trim().to_string();
	if runner.is_empty() {
		let message = DiagnosticMessage::new(m.capture("fence").unwrap().token.clone());
		let diagnostic = Diagnostic::new(DiagnosticKind::MissingRunner, message);
		lower.diagnostics.push(diagnostic);
		return;
	}
	let options = match m.capture("options").map(|capture| &capture.value) {
		Some(Value::Options(options)) => Some(options),
		_ => None,
	};
	let option = |name| match options.and_then(|options| options.get(name)) {
		Some(OptionValue::Text(text)) => text.clone(),
		_ => String::new(),
	};
	let show = option("show");
	let inputs = crate::variable::expand_paths(lower, &option("inputs"));
	let source = m.capture("source").unwrap().token.content();
	let code = source.strip_suffix('\n').unwrap_or(source).to_string();
	let file = lower.files.last().unwrap().clone();
	let request = RunRequest {
		runner: runner.clone(),
		code: code.clone(),
		inputs: inputs.split_whitespace().map(String::from).collect(),
	};
	let command = m.capture("command").unwrap().token.clone();
	let lookup = match lower.resolver.run(&file, &request) {
		Ok(lookup) => lookup,
		Err(RunError::UnknownRunner { runners }) => {
			let runners = match runners.is_empty() {
				true => "none".to_string(),
				false => (runners.iter())
					.map(|runner| format!("`{runner}`"))
					.collect::<Vec<_>>()
					.join(", "),
			};
			let kind = DiagnosticKind::UnknownRunner { runner, runners };
			let diagnostic = Diagnostic::new(kind, DiagnosticMessage::new(info));
			lower.diagnostics.push(diagnostic);
			return;
		}
		Err(RunError::InputNotFound { path }) => {
			let location = m.capture("options").map_or(command, |c| c.token.clone());
			let kind = DiagnosticKind::FileNotFound { path };
			let diagnostic = Diagnostic::new(kind, DiagnosticMessage::new(location));
			lower.diagnostics.push(diagnostic);
			return;
		}
	};
	for (path, hash) in &lookup.inputs {
		lower.depend(Dependency::File {
			path: path.clone(),
			hash: hash.clone(),
		});
	}
	let completed = lookup
		.result
		.as_ref()
		.is_some_and(|result| result.status == RunStatus::Completed);
	lower.depend(Dependency::Run {
		key: lookup.key.clone(),
		completed,
	});
	if let Some(RunResult {
		status: RunStatus::Failed,
		message,
		..
	}) = &lookup.result
	{
		let kind = DiagnosticKind::RunFailed {
			message: message.clone().unwrap_or_default(),
		};
		let diagnostic = Diagnostic::new(kind, DiagnosticMessage::new(command.clone()));
		lower.diagnostics.push(diagnostic);
	}

	let name = m.capture("name").map(|capture| capture.token.clone());
	if let Some(name) = &name {
		let first = (lower.runs.iter())
			.filter_map(|run| run.name.as_ref())
			.find(|first| first.content() == name.content());
		if let Some(first) = first {
			let kind = DiagnosticKind::DuplicateRunName {
				name: name.content().into(),
			};
			let diagnostic = Diagnostic::new(kind, DiagnosticMessage::new(name.clone()))
				.with(DiagnosticMessage::fluent(first.clone(), "first"));
			lower.diagnostics.push(diagnostic);
		}
	}

	if matches!(show.as_str(), "code" | "both") {
		let kind = ElementKind::CodeBlock {
			language: Some(runner.clone()),
			code: code.clone(),
			first_line: None,
			max_lines: None,
			number: None,
		};
		lower.builder.leaf(kind, m.token.clone());
	}
	let stdout = match &lookup.result {
		Some(result) if result.status == RunStatus::Completed => result.stdout.trim_end(),
		_ => "",
	};
	if matches!(show.as_str(), "output" | "both") && !stdout.is_empty() {
		let kind = ElementKind::CodeBlock {
			language: None,
			code: stdout.to_string(),
			first_line: None,
			max_lines: None,
			number: None,
		};
		let attributes = Attributes {
			id: None,
			classes: vec![OUTPUT_CLASS.into()],
		};
		lower.builder.open_with(kind, m.token.clone(), attributes);
		lower.builder.close();
	}

	lower.runs.push(RunBlock {
		name,
		runner,
		code,
		file,
		token: m.token.clone(),
		command,
		key: lookup.key,
		result: lookup.result,
		files: lookup.files,
	});
}

/// Why a `run:` URL shows nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunUrlError {
	/// No earlier run has that name.
	Unknown { name: String },
	/// The run has no such file.
	MissingFile { name: String, file: String },
	/// `run:name` for a run that did not write exactly one file.
	Ambiguous { name: String, files: Vec<String> },
	/// The run failed, or has not run: it says so itself.
	NoResult { name: String },
}

impl RunUrlError {
	/// Its diagnostic, if it has one of its own.
	pub fn diagnostic(&self) -> Option<DiagnosticKind> {
		Some(match self {
			RunUrlError::Unknown { name } => DiagnosticKind::UnknownRun { name: name.clone() },
			RunUrlError::MissingFile { name, file } => DiagnosticKind::MissingRunOutput {
				name: name.clone(),
				file: file.clone(),
			},
			RunUrlError::Ambiguous { name, files } => DiagnosticKind::AmbiguousRunOutput {
				name: name.clone(),
				count: files.len(),
				files: (files.iter())
					.map(|file| format!("`{file}`"))
					.collect::<Vec<_>>()
					.join(", "),
			},
			RunUrlError::NoResult { .. } => return None,
		})
	}
}

impl std::fmt::Display for RunUrlError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			RunUrlError::Unknown { name } => write!(f, "no run before this is named `{name}`"),
			RunUrlError::MissingFile { name, file } => {
				write!(f, "the run `{name}` wrote no file `{file}`")
			}
			RunUrlError::Ambiguous { name, files } => write!(
				f,
				"the run `{name}` wrote {} files, not one: name the file with `run:{name}/<file>`",
				files.len()
			),
			RunUrlError::NoResult { name } => write!(f, "the run `{name}` has no result"),
		}
	}
}

/// The file a `run:` URL names (`url` without the scheme): among the files of the last run
/// of that name in the unit.
pub(crate) fn run_file<'r>(runs: &'r [RunBlock], url: &str) -> Result<&'r RunFile, RunUrlError> {
	let (name, file) = match url.split_once('/') {
		Some((name, file)) => (name, Some(file)),
		None => (url, None),
	};
	let run = (runs.iter().rev())
		.find(|run| run.name.as_ref().is_some_and(|n| n.content() == name))
		.ok_or_else(|| RunUrlError::Unknown { name: name.into() })?;
	let completed = run
		.result
		.as_ref()
		.is_some_and(|result| result.status == RunStatus::Completed);
	if !completed {
		return Err(RunUrlError::NoResult { name: name.into() });
	}
	match file {
		Some(file) => {
			(run.files.iter())
				.find(|f| f.name == file)
				.ok_or_else(|| RunUrlError::MissingFile {
					name: name.into(),
					file: file.into(),
				})
		}
		None => match run.files.as_slice() {
			[only] => Ok(only),
			files => Err(RunUrlError::Ambiguous {
				name: name.into(),
				files: files.iter().map(|f| f.name.clone()).collect(),
			}),
		},
	}
}

#[cfg(test)]
mod tests {
	use std::collections::HashMap;
	use std::sync::Arc;
	use std::time::Duration;

	use nml_diagnostics::assert_diagnostics;
	use nml_document::Dependency;
	use nml_document::ElementKind;
	use nml_render::RunResult;
	use nml_render::RunStatus;
	use nml_source::Source;
	use nml_source::Url;

	use crate::Compiled;
	use crate::MemoryResolver;
	use crate::tests::show;

	fn completed(stdout: &str, files: &[&str]) -> RunResult {
		RunResult {
			status: RunStatus::Completed,
			stdout: stdout.into(),
			stderr: String::new(),
			message: None,
			duration: Duration::from_millis(10),
			files: files.iter().map(|file| file.to_string()).collect(),
		}
	}

	fn failed(message: &str) -> RunResult {
		RunResult {
			status: RunStatus::Failed,
			stdout: String::new(),
			stderr: message.into(),
			message: Some(message.into()),
			duration: Duration::from_millis(10),
			files: Vec::new(),
		}
	}

	/// Compiles `dir/page.nml` with the runner `sh`, whose results are given by code.
	fn compile(text: &str, results: &[(&str, RunResult)], files: &[(&str, &str)]) -> Compiled {
		let source = Source::new(Url::parse("file:///dir/page.nml").unwrap(), text.into(), 0);
		let mut resolver = MemoryResolver::default();
		for (path, content) in files {
			resolver.files.insert(path.to_string(), content.to_string());
		}
		let results: HashMap<_, _> = (results.iter())
			.map(|(code, result)| (code.to_string(), result.clone()))
			.collect();
		resolver.runs.insert("sh".into(), results);
		resolver.runs.insert("R".into(), HashMap::new());
		crate::compile(
			&crate::Language::builtin(),
			Arc::new(source),
			"dir/page.nml",
			Arc::new(resolver),
		)
	}

	#[test]
	fn runs_and_their_files() {
		let compiled = compile(
			":run #plot\n```sh\nplot\n```\n\
			![A plot](run:plot)\n\n![Again](run:plot/plot.svg)\n",
			&[("plot", completed("printed\n", &["plot.svg"]))],
			&[("runs/plot/plot.svg", "<svg/>")],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let compiled_two = compile(
			":run #two\n```sh\ntwo\n```\n![Data](run:two/sub/data.png){#data}\n",
			&[("two", completed("", &["a.svg", "sub/data.png"]))],
			&[("runs/two/sub/data.png", "png")],
		);
		assert_diagnostics!(compiled_two.diagnostics, []);
		let document = &compiled_two.unit.document;
		let media: Vec<_> = (document.descendants(document.root()))
			.filter_map(|(_, element)| match &element.kind {
				ElementKind::Media { url, .. } => Some(url.clone()),
				_ => None,
			})
			.collect();
		let hash = blake3::hash(b"png").to_hex().to_string();
		// From `dir/page.html`.
		assert_eq!(media, [format!("../assets/{}.png", &hash[..16])]);
		assert_eq!(compiled_two.unit.assets.len(), 1);
		assert_eq!(compiled_two.unit.assets[0].path, "runs/two/sub/data.png");
		// Named media from runs are referenceable, with their file.
		let export = &compiled_two.unit.exports[0];
		assert_eq!(export.property("media"), Some("runs/two/sub/data.png"));

		// The first unit: its only file, twice, is one asset.
		assert_eq!(compiled.unit.assets.len(), 1);
		let run = &compiled.runs[0];
		assert_eq!(run.name.as_ref().unwrap().content(), "plot");
		assert_eq!(run.runner, "sh");
		assert_eq!(run.code, "plot");
		assert_eq!(run.file, "dir/page.nml");
		assert_eq!(run.command.content(), ":run");
		assert_eq!(run.token.content(), ":run #plot\n```sh\nplot\n```\n");
		assert_eq!(run.result, Some(completed("printed\n", &["plot.svg"])));
		assert_eq!(
			compiled.unit.dependencies,
			[Dependency::Run {
				key: run.key.clone(),
				completed: true
			}]
		);
		// Nothing is shown by default.
		assert_eq!(
			show(&compiled.unit.document, compiled.unit.document.root()),
			r#"document[paragraph[media[]] paragraph[media[]]]"#
		);
	}

	#[test]
	fn url_errors() {
		let results = [
			("two", completed("", &["a.svg", "b.svg"])),
			("none", completed("", &[])),
			("bad", failed("boom")),
		];
		let compiled = compile(
			":run #two\n```sh\ntwo\n```\n:run #none\n```sh\nnone\n```\n\
			:run #bad\n```sh\nbad\n```\n:run #later\n```sh\nlater\n```\n\
			![](run:two)\n\n![](run:two/c.svg)\n\n![](run:none)\n\n![](run:nope)\n\n\
			![](run:bad)\n\n![](run:later/x.svg)\n",
			&results,
			&[],
		);
		assert_diagnostics!(
			compiled.diagnostics,
			[
				3017 @ 50..54,
				2027 @ 106..113,
				2026 @ 120..133,
				2027 @ 140..148,
				2025 @ 155..163,
			]
		);
		// Runs that failed or have not run show nothing, and say so themselves.
		let document = &compiled.unit.document;
		assert!(
			!(document.descendants(document.root()))
				.any(|(_, element)| matches!(element.kind, ElementKind::Media { .. }))
		);
		let later = &compiled.runs[3];
		assert_eq!(later.result, None);
		assert!(compiled.unit.dependencies.contains(&Dependency::Run {
			key: later.key.clone(),
			completed: false
		}));
	}

	#[test]
	fn shown_code_and_output() {
		let compiled = compile(
			":run[show=both]\n```sh\necho hi\n```\n:run[show=code]\n```sh\nx\n```\n\
			:run[show=output]\n```sh\nquiet\n```\n:run[show=output]\n```sh\nnot run\n```\n",
			&[
				("echo hi", completed("hi\n\n", &[])),
				("x", completed("x\n", &[])),
				("quiet", completed("", &[])),
			],
			&[],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		let blocks: Vec<_> = (document.descendants(document.root()))
			.filter_map(|(_, element)| match &element.kind {
				ElementKind::CodeBlock { language, code, .. } => Some((
					language.clone(),
					code.clone(),
					element.attributes.classes.clone(),
				)),
				_ => None,
			})
			.collect();
		assert_eq!(
			blocks,
			[
				(Some("sh".into()), "echo hi".into(), vec![]),
				(None, "hi".into(), vec![super::OUTPUT_CLASS.to_string()]),
				(Some("sh".into()), "x".into(), vec![]),
			]
		);
	}

	#[test]
	fn block_errors() {
		let compiled = compile(
			":run\n```\nx\n```\n:run\n```python\nx\n```\n\
			:run[inputs=data.csv missing.csv]\n```sh\nx\n```\n\
			:run #a\n```sh\na\n```\n:run #a\n```sh\nb\n```\n",
			&[],
			&[("dir/data.csv", "1,2")],
		);
		assert_diagnostics!(
			compiled.diagnostics,
			[2023 @ 5..8, 3016 @ 23..29, 3008 @ 40..69, 2024 @ 108..109]
		);
		assert_eq!(compiled.runs.len(), 2);
	}

	#[test]
	fn keys_and_inputs() {
		let key = |text: &str, data: &str| {
			let compiled = compile(text, &[], &[("dir/data.csv", data), ("top.csv", "t")]);
			assert_diagnostics!(compiled.diagnostics, []);
			(compiled.runs[0].key.clone(), compiled.unit.dependencies)
		};
		let (a, dependencies) = key(":run[inputs=data.csv /top.csv]\n```sh\nx\n```\n", "1");
		assert_eq!(
			dependencies[..2],
			[
				Dependency::File {
					path: "dir/data.csv".into(),
					hash: blake3::hash(b"1").to_hex().to_string(),
				},
				Dependency::File {
					path: "top.csv".into(),
					hash: blake3::hash(b"t").to_hex().to_string(),
				},
			]
		);
		// The inputs' content, the code and the runner are in the key.
		let (b, _) = key(":run[inputs=data.csv /top.csv]\n```sh\nx\n```\n", "2");
		let (c, _) = key(":run[inputs=data.csv /top.csv]\n```sh\ny\n```\n", "1");
		let (d, _) = key(":run[inputs=data.csv /top.csv]\n```R\nx\n```\n", "1");
		let (e, _) = key(
			":run[inputs=data.csv /top.csv, show=code]\n```sh\nx\n```\n",
			"1",
		);
		assert_ne!(a, b);
		assert_ne!(a, c);
		assert_ne!(a, d);
		// What it shows is not.
		assert_eq!(a, e);
	}
}
