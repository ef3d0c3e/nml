//! The `nml` command-line tool.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use clap::Subcommand;
use nml_diagnostics::Localizer;

#[derive(Debug, Parser)]
#[command(name = "nml", version, about = "Not a Markup Language")]
struct Cli {
	#[command(subcommand)]
	command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
	/// Build every page of a workspace.
	Build {
		/// The workspace root, which holds `nml.toml`.
		#[arg(default_value = ".")]
		root: PathBuf,
	},
}

fn main() -> ExitCode {
	match Cli::parse().command {
		Command::Build { root } => build(root),
	}
}

fn build(root: PathBuf) -> ExitCode {
	let report = match nml_build::build(&root) {
		Ok(report) => report,
		Err(error) => {
			eprintln!("error: {error}");
			return ExitCode::FAILURE;
		}
	};

	let localizer = Localizer::from_env();
	let mut stderr = std::io::stderr().lock();
	let color = stderr.is_terminal();
	for diagnostic in &report.diagnostics {
		// A failure to write to stderr leaves nothing to report it on.
		let _ = nml_diagnostics::render(diagnostic, &localizer, color, &mut stderr);
	}
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
