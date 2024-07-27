#![feature(char_indices_offset)]
mod cache;
mod compiler;
mod document;
mod elements;
mod lua;
mod parser;

use std::env;
use std::process::ExitCode;
use std::rc::Rc;

use compiler::compiler::Compiler;
use getopts::Options;
use parser::langparser::LangParser;
use parser::parser::Parser;
use walkdir::WalkDir;

use crate::parser::source::SourceFile;
extern crate getopts;

fn print_usage(program: &str, opts: Options) {
	let brief = format!("Usage: {} -i PATH -o PATH [options]", program);
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

NML version: 0.4\n"
	);
}

fn process(
	parser: &LangParser,
	db_path: &Option<String>,
	input: &String,
	output: &String,
	debug_opts: &Vec<String>,
	multi_mode: bool,
) -> bool {
	println!("Processing {input}...");
	// Parse
	let source = SourceFile::new(input.to_string(), None).unwrap();
	let doc = parser.parse(Rc::new(source), None);

	if debug_opts.contains(&"ast".to_string()) {
		println!("-- BEGIN AST DEBUGGING --");
		doc.content()
			.borrow()
			.iter()
			.for_each(|elem| println!("{}", (elem).to_string()));
		println!("-- END AST DEBUGGING --");
	}

	if debug_opts.contains(&"ref".to_string()) {
		println!("-- BEGIN REFERENCES DEBUGGING --");
		let sc = doc.scope().borrow();
		sc.referenceable.iter().for_each(|(name, reference)| {
			println!(" - {name}: `{:#?}`", doc.get_from_reference(reference));
		});
		println!("-- END REFERENCES DEBUGGING --");
	}
	if debug_opts.contains(&"var".to_string()) {
		println!("-- BEGIN VARIABLES DEBUGGING --");
		let sc = doc.scope().borrow();
		sc.variables.iter().for_each(|(_name, var)| {
			println!(" - `{:#?}`", var);
		});
		println!("-- END VARIABLES DEBUGGING --");
	}

	if parser.has_error() {
		println!("Compilation aborted due to errors while parsing");
		return false;
	}

	let compiler = Compiler::new(compiler::compiler::Target::HTML, db_path.clone());

	// Get output from file
	if multi_mode {
		let out_file = match doc.get_variable("compiler.output") {
			None => {
				eprintln!("Missing required variable `compiler.output` for multifile mode");
				return false;
			}
			Some(var) => output.clone() + "/" + var.to_string().as_str(),
		};

		let out = compiler.compile(doc.as_ref());
		std::fs::write(out_file, out).is_ok()
	} else {
		let out = compiler.compile(doc.as_ref());
		std::fs::write(output, out).is_ok()
	}
}

fn main() -> ExitCode {
	let args: Vec<String> = env::args().collect();
	let program = args[0].clone();

	let mut opts = Options::new();
	opts.optopt("i", "input", "Input path", "PATH");
	opts.optopt("o", "output", "Output path", "PATH");
	opts.optopt("d", "database", "Cache database location", "PATH");
	opts.optmulti("z", "debug", "Debug options", "OPTS");
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
	if !matches.opt_present("i") || !matches.opt_present("o") {
		print_usage(&program, opts);
		return ExitCode::FAILURE;
	}

	let input = matches.opt_str("i").unwrap();
	let input_meta = match std::fs::metadata(&input) {
		Ok(meta) => meta,
		Err(e) => {
			eprintln!("Unable to get metadata for input: `{input}`");
			return ExitCode::FAILURE;
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
					return ExitCode::FAILURE;
				}
			}
		}
		match std::fs::metadata(&output) {
			Ok(_) => {}
			Err(e) => {
				eprintln!("Unable to get metadata for output: `{output}`");
				return ExitCode::FAILURE;
			}
		}
	}

	let debug_opts = matches.opt_strs("z");
	let db_path = matches.opt_str("d");
	let parser = LangParser::default();

	if input_meta.is_dir() {
		if db_path.is_none() {
			eprintln!("Please specify a database (-d) for directory mode.");
		}

		let input_it = match std::fs::read_dir(&input) {
			Ok(it) => it,
			Err(e) => {
				eprintln!("Failed to read input directory `{input}`: {e}");
				return ExitCode::FAILURE;
			}
		};

		for entry in WalkDir::new(&input) {
			if let Err(err) = entry {
				eprintln!("Failed to recursively walk over input directory: {err}");
				return ExitCode::FAILURE;
			}
			match entry.as_ref().unwrap().metadata() {
				Ok(meta) => {
					if !meta.is_file() {
						continue;
					}
				}
				Err(e) => {
					eprintln!("Faield to get metadata for `{entry:#?}`");
					return ExitCode::FAILURE;
				}
			}

			let path = match entry.as_ref().unwrap().path().to_str() {
				Some(path) => path.to_string(),
				None => {
					eprintln!("Faield to convert input file `{entry:#?}` to UTF-8");
					return ExitCode::FAILURE;
				}
			};
			if !path.ends_with(".nml") {
				println!("Skipping '{path}'");
				continue;
			}

			if !process(&parser, &db_path, &path, &output, &debug_opts, true) {
				eprintln!("Processing aborted");
				return ExitCode::FAILURE;
			}
		}
	} else {
		if !process(&parser, &db_path, &input, &output, &debug_opts, false) {
			eprintln!("Processing aborted");
			return ExitCode::FAILURE;
		}
	}

	return ExitCode::SUCCESS;
}
