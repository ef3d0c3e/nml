use std::{path::{Path, PathBuf}, sync::Arc, time::{SystemTime, UNIX_EPOCH}};

use ariadne::Fmt;

use crate::{cache::cache::Cache, parser::{parser::Parser, reports::macros::*, resolver::Resolver, source::SourceFile}, unit::translation::TranslationUnit};
use crate::parser::reports::*;

use super::{compiled::CompiledUnit, compiler::{Compiler, Target}};

#[derive(Debug)]
pub enum ProcessError
{
	GeneralError(String),
	InputError(String, String),
	LinkError(Vec<Report>),
}

pub enum ProcessOutputOptions {
	/// Path to the directory
	Directory(String),
	/// Path to the output file
	File(String),
}

/// Processqueue for inputs
pub struct ProcessQueue
{
	inputs: Vec<PathBuf>,
	outputs: Vec<CompiledUnit>,

	cache: Arc<Cache>,
	project_path: String,
	parser: Parser,
	compiler: Compiler,
}

impl ProcessQueue {
	pub fn new(target: Target, db: Option<&str>, inputs: Vec<PathBuf>) -> Self
	{
		let cache = Arc::new(Cache::new(db).unwrap());
		let project_path = db
			.map(|s| { 
				let mut buf = Path::new(s).to_path_buf();
				buf.pop();
				buf.to_str().unwrap().to_string()
			})
			.unwrap_or(String::default());

		let parser = Parser::new();
		let compiler = Compiler::new(target, cache.clone());

		Self {
			inputs,
			outputs: vec![],
			cache,
			project_path,
			parser,
			compiler
		}
	}

	pub fn process(&mut self, options: ProcessOutputOptions) -> Result<Vec<CompiledUnit>, ProcessError>
	{
		match &options
		{
			ProcessOutputOptions::Directory(dir) => {

			},
			ProcessOutputOptions::File(file) => {
				if self.inputs.len() > 1
				{
					Err(ProcessError::GeneralError("Single file specified with multiple inputs. Please specify a directory instead".into()))?
				}
			},
		};

		// Init cache
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.cache.get_connection());
		con.execute(
			"CREATE TABLE IF NOT EXISTS units(
				input_file		TEXT PRIMARY KEY,
				mtime			INTEGER NOT NULL
			);", ()).unwrap();
		drop(con);

		let mut processed = vec![];
		for input in &self.inputs {
			let input_string = input
				.to_str()
				.map(|s| s.to_string())
				.ok_or(
					ProcessError::GeneralError(format!("Failed to convert {input:#?} to string"))
					)?;

			println!("Processing: {input_string} / {}", self.project_path);

			// Compute path
			let Some(local_path) = pathdiff::diff_paths(&input_string, &self.project_path) else {
				Err(ProcessError::InputError(format!("Failed to compute local path. Base=`{:#?}` Input=`{input_string}`", self.project_path), input_string))?
			};
			let Some(local_path) = local_path.to_str().map(|s| s.to_string()) else {
				Err(ProcessError::InputError(format!("Failed to translate `{local_path:#?}` to a string."), input_string))?
			};

			// Get mtime
			let meta = std::fs::metadata(input)
				.map_err(|err| ProcessError::InputError(input_string.clone(), format!("Failed to get metadata for `{input_string}`: {err}")))?;

			let mtime = meta
				.modified()
				.map(|e| e.duration_since(UNIX_EPOCH).unwrap().as_secs())
				.map_err(|err| ProcessError::InputError(input_string.clone(), format!("Unable to query modification time for `{input_string}`: {err}")))?;

			{
				let con = tokio::runtime::Runtime::new()
					.unwrap()
					.block_on(self.cache.get_connection());
				let prev_mtime = con.query_row("SELECT mtime FROM units WHERE input_file = (?1)", [local_path.to_string()], |row| Ok(row.get_unwrap::<_, u64>(0))).unwrap_or(0);

				if prev_mtime >= mtime
				{
					println!("Skipping processing of `{local_path}`");
					continue;
				}
			}

			// Create unit
			let source = Arc::new(SourceFile::new(input_string.clone(), None).unwrap());
			let mut unit = TranslationUnit::new(local_path, &self.parser, source, false, true);

			// TODO: Check if necessary to compile using mtime
			let output_file = match &options {
				ProcessOutputOptions::Directory(dir) => {
					let basename = match input_string.rfind(|c| c == '.')
					{
						Some(pos) => &input_string[0..pos],
						None => &input_string,
					};
					format!("{dir}/{basename}.html")
				},
				ProcessOutputOptions::File(file) => {
					let basename = match input_string.rfind(|c| c == '.')
					{
						Some(pos) => &input_string[0..pos],
						None => &input_string,
					};
					format!("{basename}.html")
				},
			};

			// Insert document in unit database
			let con = tokio::runtime::Runtime::new()
				.unwrap()
				.block_on(self.cache.get_connection());
			con.execute("INSERT OR REPLACE INTO
				units (input_file, mtime)
				VALUES (?1, ?2)", (unit.input_path(), 0)).unwrap();

			let Some(unit) = unit.consume(output_file) else { continue };
			println!("result={:#?}", unit.get_entry_scope());

			processed.push(unit);
		}

		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.cache.get_connection());

		// Resolve all references
		let colors = ReportColors::with_colors();
		let resolver = Resolver::new(&colors, &con, &processed)
			.map_err(|err| ProcessError::LinkError(vec![err]))?;
		for unit in &processed
		{
			// Output references
			unit.export_references(&con).expect("Failed to export");
		}
		let errors = resolver.resolve_all(&con);
		if !errors.is_empty()
		{
			return Err(ProcessError::LinkError(errors));
		}

		let time_now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
		for unit in &processed
		{
			// Insert document in unit database
			con.execute("INSERT OR REPLACE INTO
				units (input_file, mtime)
				VALUES (?1, ?2)", (unit.input_path(), time_now)).unwrap();
		}

		Ok(vec![])
	}
}
