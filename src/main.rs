mod cache;
mod compiler;
mod elements;
mod lsp;
mod parser;
mod unit;
mod util;
mod lua;
mod layout;

use std::env::{self};
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use compiler::compiler::Target;
use compiler::process::ProcessError;
use compiler::process::ProcessOptions;
use compiler::process::ProcessOutputOptions;
use compiler::process::ProcessQueue;
use getopts::Matches;
use getopts::Options;
use parser::reports::Report;
use parser::reports::ReportColors;
use util::settings::ProjectSettings;
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
) -> Result<(Vec<PathBuf>, ProcessOutputOptions, ProjectSettings), String> {
	let input = matches.opt_str("i").unwrap();
	let input_meta = match std::fs::metadata(&input) {
		Ok(meta) => meta,
		Err(e) => {
			return Err(format!("Unable to get metadata for input `{input}`: {e}"));
		}
	};
	let output = matches.opt_str("o").unwrap();
	if input_meta.is_dir() {
		// Create ouput directories
		if !std::fs::exists(&output).unwrap_or(false) {
			match std::fs::create_dir_all(&output) {
				Ok(()) => {}
				Err(err) => {
					return Err(format!(
						"Unable to create output directory `{output}`: {err}"
					));
				}
			}
		}
		match std::fs::metadata(&output) {
			Ok(output_meta) => {
				if !output_meta.is_dir() {
					return Err(format!(
						"Input is a directory, but ouput is not a directory, halting"
					));
				}
			}
			Err(e) => {
				return Err(format!("Unable to get metadata for output `{output}`: {e}"));
			}
		}
	} else if std::fs::exists(&output).unwrap_or(false) {
		let output_meta = match std::fs::metadata(&output) {
			Ok(meta) => meta,
			Err(e) => {
				return Err(format!("Unable to get metadata for output `{output}`: {e}"));
			}
		};

		if output_meta.is_dir() {
			return Err(format!(
				"Input `{input}` is a file, but output `{output}` is a directory"
			));
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
						return Err(format!("Failed to transform path to string `{db}`"));
					}
					Err(err) => {
						return Err(format!("{err}"));
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
						return Err(format!("Failed to transform path to string `{db}`"));
					}
					Err(err) => {
						return Err(format!("{err}"));
					}
				}
			}
		}
		None => None,
	};

	let mut files = vec![];
	if input_meta.is_dir() {
		if db_path.is_none() {
			return Err(format!("Directory mode requires a database (-d)"));
		}

		for entry in WalkDir::new(&input) {
			if let Err(err) = entry {
				return Err(format!(
					"Failed to recursively walk over input directory: {err}"
				));
			}
			match entry.as_ref().unwrap().metadata() {
				Ok(meta) => {
					if !meta.is_file() {
						continue;
					}
				}
				Err(e) => {
					return Err(format!("Faield to get metadata for `{entry:#?}`: {e}"));
				}
			}

			let path = match entry.as_ref().unwrap().path().to_str() {
				Some(path) => path.to_string(),
				None => {
					return Err(format!(
						"Faield to convert input file `{entry:#?}` to UTF-8"
					));
				}
			};
			if !path.ends_with(".nml") {
				continue;
			}

			files.push(std::fs::canonicalize(path).unwrap());
		}
	} else {
		// Single file mode
		files.push(std::fs::canonicalize(input).unwrap());
	}
	let mut settings = ProjectSettings::default();
	settings.db_path = db_path.unwrap_or("".into());
	settings.output_path = output
		.clone()
		.split_at(output.rfind(|c| c == '/').unwrap_or(0))
		.0
		.to_string();
	if input_meta.is_dir()
	{
		Ok((
				files,
				compiler::process::ProcessOutputOptions::Directory(settings.output_path.clone()),
				settings,
		))
	}
	else
	{
		Ok((
				files,
				compiler::process::ProcessOutputOptions::File(settings.output_path.clone()),
				settings,
		))
	}
}

fn input_project(
	matches: &Matches,
) -> Result<(Vec<PathBuf>, ProcessOutputOptions, ProjectSettings), String> {
	let settings_file = matches.opt_str("p").unwrap();
	// Get root path
	let root_path = &settings_file
		.split_at(settings_file.rfind(|c| c == '/').unwrap_or(0))
		.0
		.to_string();
	let root_meta = std::fs::metadata(root_path)
		.map_err(|e| format!("Failed to get project root metadata `{root_path}`: {e}"))?;
	if !root_meta.is_dir() {
		return Err(format!("Project root `{root_path}` is not a directory"));
	}

	let meta = match std::fs::metadata(&settings_file) {
		Ok(meta) => meta,
		Err(e) => {
			return Err(format!(
				"Unable to get metadata for file `{settings_file}`: {e}"
			))
		}
	};
	if !meta.is_file() {
		return Err(format!(
			"Project file `{settings_file}` must be a regular file"
		));
	}
	let mut settings = match fs::read(&settings_file) {
		Ok(content) => {
			let content = String::from_utf8(content)
				.map_err(|e| format!("Unable to read project file `{settings_file}`: {e}"))?;

			toml::from_str::<ProjectSettings>(content.as_str())
				.map_err(|e| format!("Failed to deserialize `{settings_file}`: {e}"))?
		}
		Err(e) => {
			return Err(format!(
				"Unable to read project file `{settings_file}`: {e}"
			))
		}
	};

	let mut files = vec![];
	for entry in WalkDir::new(&root_path) {
		if let Err(err) = entry {
			return Err(format!(
				"Failed to recursively walk over input directory: {err}"
			));
		}
		match entry.as_ref().unwrap().metadata() {
			Ok(meta) => {
				if !meta.is_file() {
					continue;
				}
			}
			Err(e) => return Err(format!("Faield to get metadata for `{entry:#?}`: {e}")),
		}

		let path = match entry.as_ref().unwrap().path().to_str() {
			Some(path) => path.to_string(),
			None => {
				return Err(format!(
					"Faield to convert input file `{entry:#?}` to UTF-8"
				))
			}
		};
		if !path.ends_with(".nml") {
			continue;
		}
		files.push(path.into());
	}
	settings.set_root_path(&root_path)?;
	Ok((
		files,
		compiler::process::ProcessOutputOptions::Directory(settings.output_path.clone()),
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
	opts.optflag("", "luals-gen", "Generates lua definitions for LuaLs");
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
	if matches.opt_present("luals-gen")
	{
		lua::doc::get_lua_docs();
		return ExitCode::SUCCESS;
	}

	let res = if matches.opt_present("p") {
		input_project(&matches)
	} else if matches.opt_present("i") && matches.opt_present("o") {
		input_manual(&matches)
	} else {
		print_usage(&program, opts);
		return ExitCode::FAILURE;
	};
	let (files, output, settings) = match res {
		Ok((files, output, settings)) => (files, output, settings),
		Err(err) => {
			eprintln!("{}", err);
			return ExitCode::FAILURE;
		}
	};

	let mut options = ProcessOptions::default();
	options.force_rebuild = matches.opt_present("force-rebuild");
	let debug_opts = matches.opt_strs("z");
	if debug_opts.contains(&"ast".into()) {
		options.debug_ast = true
	}

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
		.split_at(settings.db_path.rfind(|c| c == '/').unwrap_or(0))
		.0
		.to_string();
	let mut queue = ProcessQueue::new(Target::HTML, project_path, settings, files);
	match queue.process(output, options) {
		Ok(_) => {}
		Err(ProcessError::GeneralError(err)) => {
			eprintln!("Processing failed with error: `{err}`");
		}
		Err(ProcessError::InputError(file, err)) => {
			eprintln!("Processing failed with error: `{err}` while processing file '{file}'");
		}
		Err(ProcessError::LinkError(reports)) | Err(ProcessError::CompileError(reports)) => {
			let colors = ReportColors::with_colors();
			Report::reports_to_stdout(&colors, reports);
		}
	}
	ExitCode::SUCCESS
}
