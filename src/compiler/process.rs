use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use rusqlite::Connection;

use crate::cache::cache::Cache;
use crate::document::document::Document;
use crate::parser::langparser::LangParser;
use crate::parser::parser::ParseMode;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::source::Source;
use crate::parser::source::SourceFile;

use super::compiler::CompiledDocument;
use super::compiler::Compiler;
use super::compiler::Target;
use super::postprocess::PostProcess;

/// Parses a source file into a document
fn parse(
	parser: &LangParser,
	source: Arc<dyn Source>,
	debug_opts: &Vec<String>,
) -> Result<Box<dyn Document<'static>>, String> {
	// Parse
	//let source = SourceFile::new(input.to_string(), None).unwrap();
	let (doc, _) = parser.parse(
		ParserState::new(parser, None),
		source.clone(),
		None,
		ParseMode::default(),
	);

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

/// Takes a list of paths and processes it into a list of compiled documents
pub fn process(
	target: Target,
	files: Vec<PathBuf>,
	db_path: Option<&str>,
	force_rebuild: bool,
	debug_opts: &Vec<String>,
) -> Result<Vec<(RefCell<CompiledDocument>, Option<PostProcess>)>, String> {
	let mut compiled = vec![];

	let cache = Arc::new(Cache::new(db_path)?);
	// Initialize compiled document database
	{
		let con = cache.get_connection().await;
		CompiledDocument::init_cache(&con)
			.map_err(|err| format!("Failed to initialize cached document table: {err}"))?;
	}

	let parser = LangParser::default();
	for file in files {
		let meta = std::fs::metadata(&file)
			.map_err(|err| format!("Failed to get metadata for `{file:#?}`: {err}"))?;

		let modified = meta
			.modified()
			.map_err(|err| format!("Unable to query modification time for `{file:#?}`: {err}"))?;

		let parse_and_compile = || -> Result<(CompiledDocument, Option<PostProcess>), String> {
			// Parse
			let source = SourceFile::new(file.to_str().unwrap().to_string(), None).unwrap();
			println!("Parsing {}...", source.name());
			let doc = parse(&parser, Arc::new(source), debug_opts)?;

			// Compile
			let compiler = Compiler::new(target, cache.clone());
			let (mut compiled, postprocess) = compiler.compile(&*doc, parser.colors());

			compiled.mtime = modified.duration_since(UNIX_EPOCH).unwrap().as_secs();

			Ok((compiled, Some(postprocess)))
		};

		let (cdoc, post) = if force_rebuild {
			parse_and_compile()?
		} else {
			let cached = {
				let con = cache.get_connection().await;
				CompiledDocument::from_cache(&con, file.to_str().unwrap())
			};

			match cached {
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

	let con = cache.get_connection().await;
	for (doc, postprocess) in &compiled {
		if postprocess.is_none() {
			continue;
		}

		// Post processing
		let body = postprocess
			.as_ref()
			.unwrap()
			.apply(target, &compiled, doc)?;
		doc.borrow_mut().body = body;

		// Insert into cache
		doc.borrow().insert_cache(&con).map_err(|err| {
			format!(
				"Failed to insert compiled document from `{}` into cache: {err}",
				doc.borrow().input
			)
		})?;
	}

	Ok(compiled)
}

/// Processes sources from in-memory strings
/// This function is indented for testing
#[cfg(test)]
pub fn process_from_memory(
	target: Target,
	sources: Vec<String>,
) -> Result<Vec<(RefCell<CompiledDocument>, Option<PostProcess>)>, String> {
	let mut compiled = vec![];
	let cache = Arc::new(Cache::new(None)?);

	let parser = LangParser::default();
	for (idx, content) in sources.iter().enumerate() {
		let parse_and_compile = || -> Result<(CompiledDocument, Option<PostProcess>), String> {
			// Parse
			let source = SourceFile::with_content(format!("{idx}"), content.clone(), None);
			let doc = parse(&parser, Arc::new(source), &vec![])?;

			// Compile
			let compiler = Compiler::new(target, cache.clone());
			let (compiled, postprocess) = compiler.compile(&*doc, parser.colors());

			Ok((compiled, Some(postprocess)))
		};

		let (cdoc, post) = parse_and_compile()?;
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
			.apply(target, &compiled, doc)?;
		doc.borrow_mut().body = body;
	}

	Ok(compiled)
}
