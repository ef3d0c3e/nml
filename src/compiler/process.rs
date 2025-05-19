use std::{path::{Path, PathBuf}, sync::Arc, time::{SystemTime, UNIX_EPOCH}};

use ariadne::{Color, Fmt};
use graphviz_rust::print;

use crate::{cache::cache::Cache, parser::{parser::Parser, reports::macros::*, resolver::Resolver, source::SourceFile}, unit::translation::TranslationUnit};
use crate::parser::reports::*;

use super::{compiler::{Compiler, Target}, output};

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

/// Message for the queue
pub enum ProcessQueueMessage<'u>
{
	/// Source file being skipped
	Skipped(&'u String),
	/// Source file being parsed
	Parsing(&'u String),
	/// Unit being resolved
	Resolving(&'u TranslationUnit<'u>),
	/// Unit being compiled
	Compiling(&'u TranslationUnit<'u>),
}

/// Displays message to stdout
pub fn output_message<'u>(message: ProcessQueueMessage<'u>, perc: f64)
{
	print!("[{: >3}%] ", perc * 100.0f64);
	match message
	{
    ProcessQueueMessage::Skipped(source) => println!("{}",
		format!("Skipping '{}'", source).fg(Color::Green)),
    ProcessQueueMessage::Parsing(source) => println!("{}",
		format!("Parsing '{}'", source).fg(Color::Green)),
    ProcessQueueMessage::Resolving(unit) => println!("{} {}",
		format!("Resolving '{}'", unit.input_path()).fg(Color::Green),
		format!("[{}]", unit.reference_key()).fg(Color::Blue)),
    ProcessQueueMessage::Compiling(unit) => println!("{}",
		format!("Compiling '{}' -> '{}'", unit.input_path(), unit.output_path().unwrap()).fg(Color::Green)),
	}
}

/// Processqueue for inputs
pub struct ProcessQueue
{
	inputs: Vec<PathBuf>,
	outputs: Vec<( /* TODO */ )>,

	cache: Arc<Cache>,
	project_path: String,
	parser: Parser,
	compiler: Compiler,
}

impl ProcessQueue {
	pub fn new(target: Target, db: Option<&str>, inputs: Vec<PathBuf>) -> Self
	{
		let cache = Arc::new(Cache::new(db).unwrap());
		cache.setup_tables();
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

	pub fn process(&mut self, options: ProcessOutputOptions) -> Result<Vec<(/* TODO */)>, ProcessError>
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
		for (idx, input) in self.inputs.iter().enumerate() {
			let input_string = input
				.to_str()
				.map(|s| s.to_string())
				.ok_or(
					ProcessError::GeneralError(format!("Failed to convert {input:#?} to string"))
					)?;

			//println!("Processing: {input_string} / {}", self.project_path);

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
			let prev_mtime = self.cache.get_mtime(&local_path.to_string())
				.unwrap_or(0);

			if prev_mtime >= mtime
			{
				output_message(ProcessQueueMessage::Skipped(&input_string), (1 + idx) as f64 / self.inputs.len() as f64);
				continue;
			}
			output_message(ProcessQueueMessage::Parsing(&input_string), (1 + idx) as f64 / self.inputs.len() as f64);
			
			// Create unit
			let source = Arc::new(SourceFile::new(input_string.clone(), None).unwrap());
			let unit = TranslationUnit::new(local_path.clone(), &self.parser, source, false, true);

			let output_file = match &options {
				ProcessOutputOptions::Directory(dir) => {
					let basename = match local_path.rfind(|c| c == '.')
					{
						Some(pos) => &local_path[0..pos],
						None => &local_path,
					};
					format!("{dir}/{basename}.html")
				},
				ProcessOutputOptions::File(file) => {
					let basename = match local_path.rfind(|c| c == '.')
					{
						Some(pos) => &local_path[0..pos],
						None => &local_path,
					};
					format!("{basename}.html")
				},
			};

			let Some(unit) = unit.consume(output_file) else { continue };
			processed.push(unit);
		}

		// Insert with time 0
		self.cache.export_units(processed.iter(), 0);

		// Resolve all references
		let colors = ReportColors::with_colors();
		let resolver = Resolver::new(&colors, self.cache.clone(), &processed)
			.map_err(|err| ProcessError::LinkError(vec![err]))?;
		for (idx, unit) in processed.iter().enumerate()
		{
			output_message(ProcessQueueMessage::Resolving(unit), (1 + idx) as f64 / processed.len() as f64);
			// Output references
			unit.export_references(self.cache.clone())
				.expect("Failed to export");
		}
		let errors = resolver.resolve_all(self.cache.clone(), self.compiler.target());
		if !errors.is_empty()
		{
			return Err(ProcessError::LinkError(errors));
		}

		// Compile all units
		for (idx, unit) in processed.iter().enumerate()
		{
			output_message(ProcessQueueMessage::Compiling(unit), (1 + idx) as f64 / processed.len() as f64);
			self.compiler.compile(unit);
		}

		let time_now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
		self.cache.export_units(processed.iter(), time_now);

		Ok(vec![])
	}
}
