use std::{path::PathBuf, sync::Arc};

use crate::{cache::cache::Cache, parser::{parser::Parser, source::SourceFile, translation::TranslationUnit}};

use super::{compiled::CompiledUnit, compiler::{Compiler, Target}};

pub enum ProcessError
{
	GeneralError(String),
	InputError(String, String),
}

pub enum ProcessOutputOptions {
	Directory(String),
	File(String),
}

/// Processqueue for inputs
pub struct ProcessQueue
{
	inputs: Vec<PathBuf>,
	outputs: Vec<CompiledUnit>,
	cache: Arc<Cache>,

	parser: Parser,
	compiler: Compiler,
}

impl ProcessQueue {
	pub fn new(target: Target, db: Option<&str>, inputs: Vec<PathBuf>) -> Self
	{
		let cache = Arc::new(Cache::new(db).unwrap());

		let parser = Parser::new();
		let compiler = Compiler::new(target, cache.clone());

		Self {
			inputs,
			outputs: vec![],
			cache,
			parser,
			compiler
		}
	}

	pub fn process(&mut self, options: ProcessOutputOptions) -> Result<Vec<CompiledUnit>, ProcessError>
	{
		match options
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

		let mut compiled = vec![];
		for input in &self.inputs {
			let input_string = input
				.to_str()
				.map(|s| s.to_string())
				.ok_or(
					ProcessError::GeneralError(format!("Failed to convert {input:#?} to string"))
					)?;

			// Get mtime
			let meta = std::fs::metadata(input)
				.map_err(|err| ProcessError::InputError(input_string.clone(), format!("Failed to get metadata for `{input_string}`: {err}")))?;

			let modified = meta
				.modified()
				.map_err(|err| ProcessError::InputError(input_string.clone(), format!("Unable to query modification time for `{input_string}`: {err}")))?;

			// Create unit
			let source = Arc::new(SourceFile::new(input_string, None).unwrap());
			let mut unit = TranslationUnit::new(&self.parser, source, false, true);

			// TODO: Check if necessary to compile using mtime
			unit = unit.consume();
			println!("{:#?}", unit.get_scope());
			todo!();
			//compiled.push(self.compiler.compile(&unit));
		}
		Ok(compiled)
	}
}
