#![feature(char_indices_offset)]
mod cache;
mod compiler;
mod document;
mod elements;
mod lua;
mod parser;

use std::env;
use std::rc::Rc;

use compiler::compiler::Compiler;
use getopts::Options;
use parser::langparser::LangParser;
use parser::parser::Parser;

use crate::parser::source::SourceFile;
extern crate getopts;

fn print_usage(program: &str, opts: Options) {
	let brief = format!("Usage: {} -i FILE [options]", program);
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

fn main() {
	let args: Vec<String> = env::args().collect();
	let program = args[0].clone();

	let mut opts = Options::new();
	opts.optopt("i", "", "Input file", "FILE");
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
		return;
	}
	if matches.opt_present("h") {
		print_usage(&program, opts);
		return;
	}
	if !matches.opt_present("i") {
		print_usage(&program, opts);
		return;
	}

	let input = matches.opt_str("i").unwrap();
	let debug_opts = matches.opt_strs("z");
	let db_path = matches.opt_str("d");

	let parser = LangParser::default();

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

	if debug_opts.contains(&"ref".to_string())
	{
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
		return;
	}

	let compiler = Compiler::new(compiler::compiler::Target::HTML, db_path);
	let out = compiler.compile(doc.as_ref());

	std::fs::write("a.html", out).unwrap();
}
