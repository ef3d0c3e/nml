mod cache;
mod compiler;
mod elements;
mod lsp;
mod lua;
mod parser;
mod settings;
mod unit;

use std::env::{self};
use std::path::PathBuf;
use std::process::ExitCode;

use compiler::compiler::Target;
use compiler::process::ProcessError;
use compiler::process::ProcessOutputOptions;
use compiler::process::ProcessQueue;
use getopts::Matches;
use getopts::Options;
use graphviz_rust::print;
use parser::reports::Report;
use parser::reports::ReportColors;
use settings::settings::HtmlOutput;
use settings::settings::ProjectSettings;
use walkdir::WalkDir;

extern crate getopts;

fn print_usage(program: &str, opts: Options) {
	let brief = format!(
		"Usage: {0} [OPTIONS]\n
		Manual: {0} [OPTS] -i PATH -o PATH\n
		Project: {0} [OPTS] -p nml.toml",
		program
	);
	print!("{}", opts.usage(&brief));
}

fn print_version() {
	print!(
		"NML -- Not a Markup Language
Copyright (c) 2024
NML is licensed under the GNU Affero General Public License version 3 (AGPLv3),
under the terms of the Free Software Foundation <https://www.gnu.org/licenses/agpl-3.0.en.html>.

This program is free software; you may modify and redistribute it.
There is NO WARRANTY, to the extent permitted by law.

NML version: 0.5\n"
	);
}

fn input_manual(
	matches: &Matches,
) -> Result<(Vec<PathBuf>, ProcessOutputOptions, ProjectSettings), ExitCode> {
	let input = matches.opt_str("i").unwrap();
	let input_meta = match std::fs::metadata(&input) {
		Ok(meta) => meta,
		Err(e) => {
			eprintln!("Unable to get metadata for input `{input}`: {e}");
			return Err(ExitCode::FAILURE);
		}
	};
	let output = matches.opt_str("o").unwrap();
	if input_meta.is_dir() {
		// Create ouput directories
		if !std::fs::exists(&output).unwrap_or(false) {
			match std::fs::create_dir_all(&output) {
				Ok(()) => {}
				Err(err) => {
					eprintln!("Unable to create output directory `{output}`: {err}");
					return Err(ExitCode::FAILURE);
				}
			}
		}
		match std::fs::metadata(&output) {
			Ok(output_meta) => {
				if !output_meta.is_dir() {
					eprintln!("Input is a directory, but ouput is not a directory, halting");
					return Err(ExitCode::FAILURE);
				}
			}
			Err(e) => {
				eprintln!("Unable to get metadata for output `{output}`: {e}");
				return Err(ExitCode::FAILURE);
			}
		}
	} else if std::fs::exists(&output).unwrap_or(false) {
		let output_meta = match std::fs::metadata(&output) {
			Ok(meta) => meta,
			Err(e) => {
				eprintln!("Unable to get metadata for output `{output}`: {e}");
				return Err(ExitCode::FAILURE);
			}
		};

		if output_meta.is_dir() {
			eprintln!("Input `{input}` is a file, but output `{output}` is a directory");
			return Err(ExitCode::FAILURE);
		}
	}

	let db_path = match &matches.opt_str("d") {
		Some(db) => {
			if std::fs::exists(db).unwrap_or(false) {
				match std::fs::canonicalize(db)
					.map_err(|err| format!("Failed to cannonicalize database path `{db}`: {err}"))
					.as_ref()
					.map(|path| path.to_str())
				{
					Ok(Some(path)) => Some(path.to_string()),
					Ok(None) => {
						eprintln!("Failed to transform path to string `{db}`");
						return Err(ExitCode::FAILURE);
					}
					Err(err) => {
						eprintln!("{err}");
						return Err(ExitCode::FAILURE);
					}
				}
			} else
			// Cannonicalize parent path, then append the database name
			{
				match std::fs::canonicalize(".")
					.map_err(|err| {
						format!("Failed to cannonicalize database parent path `{db}`: {err}")
					})
					.map(|path| path.join(db))
					.as_ref()
					.map(|path| path.to_str())
				{
					Ok(Some(path)) => Some(path.to_string()),
					Ok(None) => {
						eprintln!("Failed to transform path to string `{db}`");
						return Err(ExitCode::FAILURE);
					}
					Err(err) => {
						eprintln!("{err}");
						return Err(ExitCode::FAILURE);
					}
				}
			}
		}
		None => None,
	};

	let mut files = vec![];
	if input_meta.is_dir() {
		if db_path.is_none() {
			eprintln!("Directory mode requires a database (-d)");
			return Err(ExitCode::FAILURE);
		}

		for entry in WalkDir::new(&input) {
			if let Err(err) = entry {
				eprintln!("Failed to recursively walk over input directory: {err}");
				return Err(ExitCode::FAILURE);
			}
			match entry.as_ref().unwrap().metadata() {
				Ok(meta) => {
					if !meta.is_file() {
						continue;
					}
				}
				Err(e) => {
					eprintln!("Faield to get metadata for `{entry:#?}`: {e}");
					return Err(ExitCode::FAILURE);
				}
			}

			let path = match entry.as_ref().unwrap().path().to_str() {
				Some(path) => path.to_string(),
				None => {
					eprintln!("Faield to convert input file `{entry:#?}` to UTF-8");
					return Err(ExitCode::FAILURE);
				}
			};
			if !path.ends_with(".nml") {
				println!("Skipping '{path}'");
				continue;
			}

			files.push(std::fs::canonicalize(path).unwrap());
		}
	} else {
		// Single file mode
		files.push(std::fs::canonicalize(input).unwrap());
	}
	let mut settings = ProjectSettings::default();
	settings.db_path = db_path.unwrap_or("nml.db".into());
	settings.output_path = Some(
		output
			.clone()
			.split_at(output.rfind(|c| c == '/').unwrap_or(0)).0.to_string(),
	);
	println!("set={settings:#?}");
	Ok((
		files,
		compiler::process::ProcessOutputOptions::Directory(
			settings.output_path.as_ref().unwrap().clone(),
		),
		settings,
	))
}

fn main() -> ExitCode {
	let args: Vec<String> = env::args().collect();
	let program = args[0].clone();

	let mut opts = Options::new();
	opts.optopt("i", "input", "Input path", "PATH");
	opts.optopt("p", "project", "Project file", "PATH");
	opts.optopt("o", "output", "Output path", "PATH");
	opts.optopt("d", "database", "Cache database location", "PATH");
	opts.optflag("", "force-rebuild", "Force rebuilding of cached documents");
	opts.optmulti("z", "debug", "Debug options", "[ast,ref,var]");
	opts.optflag("h", "help", "Print this help menu");
	opts.optflag("v", "version", "Print program version and licenses");

	let matches = match opts.parse(&args[1..]) {
		Ok(m) => m,
		Err(f) => {
			panic!("{}", f.to_string())
		}
	};
	if matches.opt_present("v") {
		print_version();
		return ExitCode::SUCCESS;
	}
	if matches.opt_present("h") {
		print_usage(&program, opts);
		return ExitCode::SUCCESS;
	}
	if (!matches.opt_present("i") || !matches.opt_present("o")) && !matches.opt_present("p") {
		print_usage(&program, opts);
		return ExitCode::FAILURE;
	}
	let force_rebuild = matches.opt_present("force-rebuild");
	let debug_opts = matches.opt_strs("z");

	let Ok((files, output, settings)) = input_manual(&matches) else {
		return ExitCode::FAILURE;
	};

	// Check that all files have a valid unicode path
	for file in &files {
		if file.to_str().is_none() {
			eprintln!("Invalid unicode for file: `{file:#?}`");
			return ExitCode::FAILURE;
		}
	}
	println!("files={files:#?}");

	let project_path = settings
		.db_path
		.clone()
		.split_at(settings.db_path.rfind(|c| c == '/').unwrap_or(0)).0.to_string();
	let mut queue = ProcessQueue::new(Target::HTML, project_path, settings, files);
	match queue.process(output) {
		Ok(_) => {}
		Err(ProcessError::GeneralError(err)) => {
			eprintln!("Processing failed with error: `{err}`");
		}
		Err(ProcessError::InputError(file, err)) => {
			eprintln!("Processing failed with error: `{err}` while processing file '{file}'");
		}
		Err(ProcessError::LinkError(reports)) => {
			let colors = ReportColors::with_colors();
			Report::reports_to_stdout(&colors, reports);
		}
	}

	ExitCode::SUCCESS
}
