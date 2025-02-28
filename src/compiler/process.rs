use std::{path::{Path, PathBuf}, sync::Arc};

use crate::{cache::cache::Cache, document::references::Refname, parser::{parser::Parser, resolver::Resolver, source::SourceFile, translation::TranslationUnit}};

use super::{compiled::CompiledUnit, compiler::{Compiler, Target}};

#[derive(Debug)]
pub enum ProcessError
{
	GeneralError(String),
	InputError(String, String),
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

		println!("db_path={inputs:#?}");
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
			println!("local={local_path:#?}\ninput={input_string}\nproj={:#?}", self.project_path);

			// Get mtime
			let meta = std::fs::metadata(input)
				.map_err(|err| ProcessError::InputError(input_string.clone(), format!("Failed to get metadata for `{input_string}`: {err}")))?;

			let modified = meta
				.modified()
				.map_err(|err| ProcessError::InputError(input_string.clone(), format!("Unable to query modification time for `{input_string}`: {err}")))?;

			// Create unit
			let source = Arc::new(SourceFile::new(input_string.clone(), None).unwrap());
			let mut unit = TranslationUnit::new(local_path, &self.parser, source, false, true);

			// TODO: Check if necessary to compile using mtime
			let output_file = match &options {
				ProcessOutputOptions::Directory(dir) => {
					let basename = match input_string.find(|c| c == '.')
					{
						Some(pos) => &input_string[0..pos],
						None => &input_string,
					};
					format!("{dir}/{basename}.html")
				},
				ProcessOutputOptions::File(file) => {
					let basename = match input_string.find(|c| c == '.')
					{
						Some(pos) => &input_string[0..pos],
						None => &input_string,
					};
					format!("{basename}.html")
				},
			};
			unit = unit.consume(output_file);
			println!("{:#?}", unit.get_scope());
			processed.push(unit);
			//todo!();
			//compiled.push(self.compiler.compile(&unit));
		}

		// Create resolver
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.cache.get_connection());
		let resolver = match Resolver::new(con, &processed) {
			Ok(resolver) => resolver,
			Err(err) => return Err(ProcessError::GeneralError(format!("Failed to construct resolver: {err}"))),
		};
		let con = tokio::runtime::Runtime::new()
			.unwrap()
			.block_on(self.cache.get_connection());
		let refname = Refname::try_from("source.nml#test").unwrap();
		let res = resolver.resolve_reference(con, &processed[0], &refname);
		println!("For {refname:#?} = {res:#?}");
		Ok(vec![])
	}
}
