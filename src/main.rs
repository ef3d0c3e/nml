mod cache;
mod compiler;
mod document;
mod elements;
mod lua;
mod parser;

use std::cell::RefCell;
use std::env;
use std::io::BufWriter;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::UNIX_EPOCH;

use compiler::compiler::CompiledDocument;
use compiler::compiler::Compiler;
use compiler::compiler::Target;
use compiler::navigation::create_navigation;
use compiler::postprocess::PostProcess;
use document::document::Document;
use getopts::Options;
use parser::langparser::LangParser;
use parser::parser::Parser;
use parser::parser::ParserState;
use rusqlite::Connection;
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

fn parse(
	parser: &LangParser,
	input: &str,
	debug_opts: &Vec<String>,
) -> Result<Box<dyn Document<'static>>, String> {
	println!("Parsing {input}...");

	// Parse
	let source = SourceFile::new(input.to_string(), None).unwrap();
	let (doc, _) = parser.parse(ParserState::new(parser, None), Rc::new(source), None);

	if debug_opts.contains(&"ast".to_string()) {
		println!("-- BEGIN AST DEBUGGING --");
		doc.content()
			.borrow()
			.iter()
			.for_each(|elem| println!("{elem:#?}"));
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
		return Err("Parsing failed due to errors while parsing".to_string());
	}

	Ok(doc)
}

fn process(
	target: Target,
	files: Vec<PathBuf>,
	db_path: &Option<String>,
	force_rebuild: bool,
	debug_opts: &Vec<String>,
) -> Result<Vec<(RefCell<CompiledDocument>, Option<PostProcess>)>, String> {
	let mut compiled = vec![];

	let current_dir = std::env::current_dir()
		.map_err(|err| format!("Unable to get the current working directory: {err}"))?;

	let con = db_path
		.as_ref()
		.map_or(Connection::open_in_memory(), |path| Connection::open(path))
		.map_err(|err| format!("Unable to open connection to the database: {err}"))?;
	CompiledDocument::init_cache(&con)
		.map_err(|err| format!("Failed to initialize cached document table: {err}"))?;

	let parser = LangParser::default();
	for file in files {
		let meta = std::fs::metadata(&file)
			.map_err(|err| format!("Failed to get metadata for `{file:#?}`: {err}"))?;

		let modified = meta
			.modified()
			.map_err(|err| format!("Unable to query modification time for `{file:#?}`: {err}"))?;

		// Move to file's directory
		let file_parent_path = file
			.parent()
			.ok_or(format!("Failed to get parent path for `{file:#?}`"))?;
		std::env::set_current_dir(file_parent_path)
			.map_err(|err| format!("Failed to move to path `{file_parent_path:#?}`: {err}"))?;

		let parse_and_compile = || -> Result<(CompiledDocument, Option<PostProcess>), String> {
			// Parse
			let doc = parse(&parser, file.to_str().unwrap(), debug_opts)?;

			// Compile
			let compiler = Compiler::new(target, db_path.clone());
			let (mut compiled, postprocess) = compiler.compile(&*doc);

			compiled.mtime = modified.duration_since(UNIX_EPOCH).unwrap().as_secs();

			Ok((compiled, Some(postprocess)))
		};

		let (cdoc, post) = if force_rebuild {
			parse_and_compile()?
		} else {
			match CompiledDocument::from_cache(&con, file.to_str().unwrap()) {
				Some(compiled) => {
					if compiled.mtime < modified.duration_since(UNIX_EPOCH).unwrap().as_secs() {
						parse_and_compile()?
					} else {
						(compiled, None)
					}
				}
				None => parse_and_compile()?,
			}
		};

		compiled.push((RefCell::new(cdoc), post));
	}

	for (doc, postprocess) in &compiled {
		if postprocess.is_none() {
			continue;
		}

		// Post processing
		let body = postprocess
			.as_ref()
			.unwrap()
			.apply(target, &compiled, &doc)?;
		doc.borrow_mut().body = body;

		// Insert into cache
		doc.borrow().insert_cache(&con).map_err(|err| {
			format!(
				"Failed to insert compiled document from `{}` into cache: {err}",
				doc.borrow().input
			)
		})?;
	}

	std::env::set_current_dir(current_dir)
		.map_err(|err| format!("Failed to set current directory: {err}"))?;

	Ok(compiled)
}

fn main() -> ExitCode {
	let args: Vec<String> = env::args().collect();
	let program = args[0].clone();

	let mut opts = Options::new();
	opts.optopt("i", "input", "Input path", "PATH");
	opts.optopt("o", "output", "Output path", "PATH");
	opts.optopt("d", "database", "Cache database location", "PATH");
	opts.optflag("", "force-rebuild", "Force rebuilding of cached documents");
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
			eprintln!("Unable to get metadata for input `{input}`: {e}");
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
			Ok(output_meta) => {
				if !output_meta.is_dir() {
					eprintln!("Input is a directory, but ouput is not a directory, halting");
					return ExitCode::FAILURE;
				}
			}
			Err(e) => {
				eprintln!("Unable to get metadata for output `{output}`: {e}");
				return ExitCode::FAILURE;
			}
		}
	} else if std::fs::exists(&output).unwrap_or(false) {
		let output_meta = match std::fs::metadata(&output) {
			Ok(meta) => meta,
			Err(e) => {
				eprintln!("Unable to get metadata for output `{output}`: {e}");
				return ExitCode::FAILURE;
			}
		};

		if output_meta.is_dir() {
			eprintln!("Input `{input}` is a file, but output `{output}` is a directory");
			return ExitCode::FAILURE;
		}
	}

	let db_path = match matches.opt_str("d") {
		Some(db) => {
			if std::fs::exists(&db).unwrap_or(false) {
				match std::fs::canonicalize(&db)
					.map_err(|err| format!("Failed to cannonicalize database path `{db}`: {err}"))
					.as_ref()
					.map(|path| path.to_str())
				{
					Ok(Some(path)) => Some(path.to_string()),
					Ok(None) => {
						eprintln!("Failed to transform path to string `{db}`");
						return ExitCode::FAILURE;
					}
					Err(err) => {
						eprintln!("{err}");
						return ExitCode::FAILURE;
					}
				}
			} else
			// Cannonicalize parent path, then append the database name
			{
				match std::fs::canonicalize(".")
					.map_err(|err| {
						format!("Failed to cannonicalize database parent path `{db}`: {err}")
					})
					.map(|path| path.join(&db))
					.as_ref()
					.map(|path| path.to_str())
				{
					Ok(Some(path)) => Some(path.to_string()),
					Ok(None) => {
						eprintln!("Failed to transform path to string `{db}`");
						return ExitCode::FAILURE;
					}
					Err(err) => {
						eprintln!("{err}");
						return ExitCode::FAILURE;
					}
				}
			}
		}
		None => None,
	};
	let force_rebuild = matches.opt_present("force-rebuild");
	let debug_opts = matches.opt_strs("z");

	let mut files = vec![];
	if input_meta.is_dir() {
		if db_path.is_none() {
			eprintln!("Directory mode requires a database (-d)");
			return ExitCode::FAILURE;
		}

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
					eprintln!("Faield to get metadata for `{entry:#?}`: {e}");
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

			files.push(std::fs::canonicalize(path).unwrap());
		}
	} else {
		// Single file mode
		files.push(std::fs::canonicalize(input).unwrap());
	}

	// Check that all files have a valid unicode path
	for file in &files {
		if file.to_str().is_none() {
			eprintln!("Invalid unicode for file: `{file:#?}`");
			return ExitCode::FAILURE;
		}
	}

	// Parse, compile using the cache
	let processed = match process(Target::HTML, files, &db_path, force_rebuild, &debug_opts) {
		Ok(processed) => processed,
		Err(e) => {
			eprintln!("{e}");
			return ExitCode::FAILURE;
		}
	};

	if input_meta.is_dir()
	// Batch mode
	{
		// Build navigation
		let navigation = match create_navigation(&processed) {
			Ok(nav) => nav,
			Err(e) => {
				eprintln!("{e}");
				return ExitCode::FAILURE;
			}
		};

		// Output
		for (doc, _) in &processed {
			let out_path = match doc
				.borrow()
				.get_variable("compiler.output")
				.or(input_meta.is_file().then_some(&output))
			{
				Some(path) => path.clone(),
				None => {
					eprintln!("Unable to get output file for `{}`", doc.borrow().input);
					continue;
				}
			};

			let nav = navigation.compile(Target::HTML, &doc);
			let file = std::fs::File::create(output.clone() + "/" + out_path.as_str()).unwrap();

			let mut writer = BufWriter::new(file);

			write!(
				writer,
				"{}{}{}{}",
				doc.borrow().header,
				nav,
				doc.borrow().body,
				doc.borrow().footer
			)
			.unwrap();
			writer.flush().unwrap();
		}
	} else
	// Single file
	{
		for (doc, _) in &processed {
			let file = std::fs::File::create(output.clone()).unwrap();

			let mut writer = BufWriter::new(file);

			write!(
				writer,
				"{}{}{}",
				doc.borrow().header,
				doc.borrow().body,
				doc.borrow().footer
			)
			.unwrap();
			writer.flush().unwrap();
		}
	}

	return ExitCode::SUCCESS;
}
