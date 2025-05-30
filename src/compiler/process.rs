use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use ariadne::Color;
use ariadne::Fmt;
use graphviz_rust::print;

use crate::cache::cache::Cache;
use crate::parser::parser::Parser;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::resolver::Resolver;
use crate::parser::source::SourceFile;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;
use util::settings::ProjectSettings;

use super::compiler::Compiler;
use super::compiler::Target;

#[derive(Default)]
pub struct ProcessOptions
{
	pub debug_ast: bool,
}

#[derive(Debug)]
pub enum ProcessError {
	GeneralError(String),
	InputError(String, String),
	LinkError(Vec<Report>),
	CompileError(Vec<Report>),
}

pub enum ProcessOutputOptions {
	/// Path to the directory
	Directory(String),
	/// Path to the output file
	File(String),
}

/// Message for the queue
pub enum ProcessQueueMessage<'u> {
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
pub fn output_message<'u>(message: ProcessQueueMessage<'u>, perc: f64) {
	print!("[{: >3.0}%] ", perc * 100.0f64);
	match message {
		ProcessQueueMessage::Skipped(source) => {
			println!("{}", format!("Skipping '{}'", source).fg(Color::Green))
		}
		ProcessQueueMessage::Parsing(source) => {
			println!("{}", format!("Parsing '{}'", source).fg(Color::Green))
		}
		ProcessQueueMessage::Resolving(unit) => println!(
			"{} {}",
			format!("Resolving '{}'", unit.input_path()).fg(Color::Green),
			format!("[{}]", unit.reference_key()).fg(Color::Blue)
		),
		ProcessQueueMessage::Compiling(unit) => println!(
			"{}",
			format!(
				"Compiling '{}' -> '{}'",
				unit.input_path(),
				unit.output_path().unwrap()
			)
			.fg(Color::Green)
		),
	}
}

/// Processqueue for inputs
pub struct ProcessQueue {
	settings: ProjectSettings,
	inputs: Vec<PathBuf>,
	outputs: Vec<()>,

	cache: Arc<Cache>,
	project_path: String,
	parser: Parser,
	compiler: Compiler,
}

impl ProcessQueue {
	pub fn new(target: Target, project_path: String, settings: ProjectSettings, inputs: Vec<PathBuf>) -> Self {
		let cache = Arc::new(Cache::new(settings.db_path.as_str()).unwrap());
		cache.setup_tables();

		let parser = Parser::new();
		let compiler = Compiler::new(target, cache.clone());

		Self {
			settings,
			inputs,
			outputs: vec![],
			cache,
			project_path,
			parser,
			compiler,
		}
	}

	pub fn process(&mut self, output: ProcessOutputOptions, options: ProcessOptions) -> Result<Vec<()>, ProcessError> {
		match &output {
			ProcessOutputOptions::Directory(dir) => {}
			ProcessOutputOptions::File(file) => {
				if self.inputs.len() > 1 {
					Err(ProcessError::GeneralError("Single file specified with multiple inputs. Please specify a directory instead".into()))?
				}
			}
		};

		let mut processed = vec![];
		for (idx, input) in self.inputs.iter().enumerate() {
			let input_string =
				input
					.to_str()
					.map(|s| s.to_string())
					.ok_or(ProcessError::GeneralError(format!(
						"Failed to convert {input:#?} to string"
					)))?;

			// Compute path
			let Some(local_path) = pathdiff::diff_paths(&input_string, &self.project_path) else {
				Err(ProcessError::InputError(
					format!(
						"Failed to compute local path. Base=`{:#?}` Input=`{input_string}`",
						self.project_path
					),
					input_string,
				))?
			};
			let Some(local_path) = local_path.to_str().map(|s| s.to_string()) else {
				Err(ProcessError::InputError(
					format!("Failed to translate `{local_path:#?}` to a string."),
					input_string,
				))?
			};

			// Get mtime
			let meta = std::fs::metadata(input).map_err(|err| {
				ProcessError::InputError(
					input_string.clone(),
					format!("Failed to get metadata for `{input_string}`: {err}"),
				)
			})?;

			let mtime = meta
				.modified()
				.map(|e| e.duration_since(UNIX_EPOCH).unwrap().as_secs())
				.map_err(|err| {
					ProcessError::InputError(
						input_string.clone(),
						format!("Unable to query modification time for `{input_string}`: {err}"),
					)
				})?;
			let prev_mtime = self.cache.get_mtime(&local_path.to_string()).unwrap_or(0);

			if prev_mtime >= mtime {
				output_message(
					ProcessQueueMessage::Skipped(&input_string),
					(1 + idx) as f64 / self.inputs.len() as f64,
				);
				continue;
			}
			output_message(
				ProcessQueueMessage::Parsing(&input_string),
				(1 + idx) as f64 / self.inputs.len() as f64,
			);

			// Create unit
			let source = Arc::new(SourceFile::new(input_string.clone(), None).unwrap());
			let unit = TranslationUnit::new(local_path.clone(), &self.parser, source, false, true);

			let output_file = match &output {
				ProcessOutputOptions::Directory(dir) => {
					let basename = match local_path.rfind(|c| c == '.') {
						Some(pos) => &local_path[0..pos],
						None => &local_path,
					};
					format!("{dir}/{basename}.html")
				}
				ProcessOutputOptions::File(file) => {
					let basename = match local_path.rfind(|c| c == '.') {
						Some(pos) => &local_path[0..pos],
						None => &local_path,
					};
					format!("{basename}.html")
				}
			};

			let Some(unit) = unit.consume(output_file) else {
				continue;
			};
			if options.debug_ast {
				println!("{:#?}", unit.get_entry_scope());
			}
			processed.push(unit);
		}

		// Insert with time 0
		self.cache.export_units(processed.iter(), 0);

		// Resolve all references
		let colors = ReportColors::with_colors();
		let resolver = Resolver::new(&colors, self.cache.clone(), &processed)
			.map_err(|err| ProcessError::LinkError(vec![err]))?;
		resolver.resolve_links(self.cache.clone(), self.compiler.target());
		for (idx, unit) in processed.iter().enumerate() {
			output_message(
				ProcessQueueMessage::Resolving(unit),
				(1 + idx) as f64 / processed.len() as f64,
			);
			// Output references
			unit.export_references(self.cache.clone())
				.expect("Failed to export");
		}
		let dependencies = resolver
			.resolve_references(self.cache.clone(), self.compiler.target())
			.map_err(|err| ProcessError::LinkError(err))?;
		let missing = self.cache.export_dependencies(&dependencies);
		if !missing.is_empty() {
			let mut reports = vec![];
			missing.iter().for_each(|(unit_file, list)| {
				for item in list
				{
					let source = Arc::new(SourceFile::new(format!("{}/{unit_file}", self.project_path), None).unwrap());
					reports.push(make_err!(
						source.clone(),
						"Missing references".into(),
						span(
							item.range.clone(),
							format!(
								"Reference `{}` no longer exists",
								(&item.depends_for).fg(Color::Blue),
							)
						)
							));
				}
			});
			return Err(ProcessError::LinkError(reports))
		}

		// Apply settings
		for unit in &mut processed
		{
			unit.update_settings(self.settings.clone());
		}

		// Compile all units
		let mut reports = vec![];
		for (idx, unit) in processed.iter().enumerate() {
			output_message(
				ProcessQueueMessage::Compiling(unit),
				(1 + idx) as f64 / processed.len() as f64,
			);
			match self.compiler.compile(unit) {
				Ok(_) => todo!(),
				Err(err) => reports.extend(err)
			}
		}
		if !reports.is_empty() { return Err(ProcessError::CompileError(reports)) }

		let time_now = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap()
			.as_secs();
		self.cache.export_units(processed.iter(), time_now);

		Ok(vec![])
	}
}
