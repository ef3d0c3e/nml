//! The `nml` command-line tool.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use clap::Subcommand;
use nml_diagnostics::Diagnostic;
use nml_diagnostics::Localizer;

mod serve;

#[derive(Debug, Parser)]
#[command(name = "nml", version, about = "Not a Markup Language")]
struct Cli {
	#[command(subcommand)]
	command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
	/// Set up a workspace: an `nml.toml` if it has none, and the Lua declarations for LuaLS.
	Init {
		/// The workspace root.
		#[arg(default_value = ".")]
		root: PathBuf,
	},
	/// Run the language server on stdin and stdout.
	Lsp,
	/// Build every page of a workspace, or one file without a workspace.
	Build {
		/// The workspace root, which holds `nml.toml`, or an `.nml` file.
		#[arg(default_value = ".")]
		root: PathBuf,
		/// For a file: where to write its page, instead of next to it.
		#[arg(short, long)]
		output: Option<PathBuf>,
	},
	/// Preview a workspace in the browser, as its files change.
	/// Write the code blocks that have `tangle=path` to their files.
	Tangle {
		/// The workspace root, which holds `nml.toml`.
		#[arg(default_value = ".")]
		root: PathBuf,
	},
	Serve {
		/// The workspace root, which holds `nml.toml`.
		#[arg(default_value = ".")]
		root: PathBuf,
		/// The port on 127.0.0.1; a free one if not given.
		#[arg(short, long, default_value_t = 0)]
		port: u16,
		/// Open the preview in the browser.
		#[arg(long)]
		open: bool,
	},
}

fn main() -> ExitCode {
	match Cli::parse().command {
		Command::Init { root } => init(root),
		Command::Build { root, output } => build(root, output),
		Command::Serve { root, port, open } => serve::serve(&root, port, open),
		Command::Tangle { root } => tangle(root),
		Command::Lsp => match nml_lsp::run_stdio() {
			Ok(()) => ExitCode::SUCCESS,
			Err(error) => {
				eprintln!("error: {error}");
				ExitCode::FAILURE
			}
		},
	}
}

/// A starting `nml.toml`.
const DEFAULT_CONFIG: &str = r#"# Which .nml files are pages; the others are libraries.
pages = ["**/*.nml"]
exclude = []

[html]
language = "en"
"#;

fn init(root: PathBuf) -> ExitCode {
	let config = root.join(nml_workspace::CONFIG_FILE);
	let result = std::fs::create_dir_all(&root)
		.and_then(|()| match config.exists() {
			true => Ok(()),
			false => std::fs::write(&config, DEFAULT_CONFIG),
		})
		.and_then(|()| nml_lua::write_types(&root))
		.and_then(|()| nml_lua::write_std(&root));
	match result {
		Ok(()) => {
			eprintln!("workspace ready in {}", root.display());
			ExitCode::SUCCESS
		}
		Err(error) => {
			eprintln!("error: {error}");
			ExitCode::FAILURE
		}
	}
}

fn build(root: PathBuf, output: Option<PathBuf>) -> ExitCode {
	let result = if root.is_file() {
		nml_build::build_file(&root, output.as_deref())
	} else if output.is_some() {
		eprintln!(
			"error: `--output` is for building one file; a workspace sets its output in `nml.toml`"
		);
		return ExitCode::FAILURE;
	} else {
		nml_build::build(&root)
	};
	let report = match result {
		Ok(report) => report,
		Err(error) => {
			eprintln!("error: {error}");
			return ExitCode::FAILURE;
		}
	};

	print_diagnostics(&report.diagnostics);
	eprintln!(
		"{} parsed, {} page(s) written, {} failed",
		report.parsed.len(),
		report.written.len(),
		report.failed.len()
	);
	if report.has_errors() {
		ExitCode::FAILURE
	} else {
		ExitCode::SUCCESS
	}
}

fn tangle(root: PathBuf) -> ExitCode {
	let report = match nml_build::tangle(&root) {
		Ok(report) => report,
		Err(error) => {
			eprintln!("error: {error}");
			return ExitCode::FAILURE;
		}
	};
	print_diagnostics(&report.diagnostics);
	for path in &report.written {
		eprintln!("wrote {path}");
	}
	eprintln!(
		"{} file(s) written, {} unchanged",
		report.written.len(),
		report.unchanged.len()
	);
	if report.has_errors() {
		ExitCode::FAILURE
	} else {
		ExitCode::SUCCESS
	}
}

/// Writes diagnostics to stderr, in color on a terminal.
fn print_diagnostics(diagnostics: &[Diagnostic]) {
	let localizer = Localizer::from_env();
	let mut stderr = std::io::stderr().lock();
	let color = stderr.is_terminal();
	for diagnostic in diagnostics {
		// A failure to write to stderr leaves nothing to report it on.
		let _ = nml_diagnostics::render(diagnostic, &localizer, color, &mut stderr);
	}
}
