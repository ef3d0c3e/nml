use std::path::Path;
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
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;
use util::settings::ProjectSettings;

use super::compiler::Compiler;
use super::compiler::Target;

#[derive(Default)]
pub struct ProcessOptions {
	pub debug_ast: bool,
	pub force_rebuild: bool,
}

#[derive(Debug)]
pub enum ProcessError {
	GeneralError(String),
	InputError(String, PathBuf),
	LinkError(Vec<Report>),
	CompileError(Vec<Report>),
}

pub enum ProcessOutputOptions {
	/// Path to the directory
	Directory(PathBuf),
	/// Path to the output file
	File(PathBuf),
}

/// Message for the queue
pub enum ProcessQueueMessage<'u> {
	/// Source file being skipped
	Skipped(&'u Path),
	/// Source file being parsed
	Parsing(&'u Path),
	/// Unit being resolved
	Resolving(&'u TranslationUnit),
	/// Unit being compiled
	Compiling(&'u TranslationUnit),
}

/// Displays message to stdout
pub fn output_message<'u>(message: ProcessQueueMessage<'u>, perc: f64) {
	print!("[{: >3.0}%] ", perc * 100.0f64);
	match message {
		ProcessQueueMessage::Skipped(source) => {
			println!(
				"{}",
				format!("Skipping '{}'", source.display()).fg(Color::Green)
			)
		}
		ProcessQueueMessage::Parsing(source) => {
			println!(
				"{}",
				format!("Parsing '{}'", source.display()).fg(Color::Green)
			)
		}
		ProcessQueueMessage::Resolving(unit) => println!(
			"{} {}",
			format!("Resolving '{}'", unit.input_path().display()).fg(Color::Green),
			format!("[{}]", unit.reference_key()).fg(Color::Blue)
		),
		ProcessQueueMessage::Compiling(unit) => println!(
			"{}",
			format!(
				"Compiling '{}' -> '{}'",
				unit.input_path().display(),
				unit.output_path().unwrap().display()
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
	project_path: PathBuf,
	parser: Arc<Parser>,
	compiler: Compiler,
}

impl ProcessQueue {
	pub fn new(
		target: Target,
		project_path: PathBuf,
		settings: ProjectSettings,
		inputs: Vec<PathBuf>,
	) -> Self {
		let cache = Arc::new(Cache::new(&settings.db_path).unwrap());
		cache.setup_tables();

		let parser = Arc::new(Parser::new());
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

	pub fn process(
		&mut self,
		output: ProcessOutputOptions,
		options: ProcessOptions,
	) -> Result<Vec<()>, ProcessError> {
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
			// Compute path
			let Some(local_path) = pathdiff::diff_paths(&input, &self.project_path) else {
				Err(ProcessError::InputError(
					format!(
						"Failed to compute local path. Base=`{}` Input=`{}`",
						self.project_path.display(),
						input.display()
					),
					PathBuf::from(input),
				))?
			};
			// Get mtime
			let meta = std::fs::metadata(input).map_err(|err| {
				ProcessError::InputError(
					format!("Failed to get metadata for `{}`: {err}", input.display()),
					PathBuf::from(input),
				)
			})?;

			let mtime = meta
				.modified()
				.map(|e| e.duration_since(UNIX_EPOCH).unwrap().as_secs())
				.map_err(|err| {
					ProcessError::InputError(
						format!(
							"Unable to query modification time for `{}`: {err}",
							input.display()
						),
						PathBuf::from(input),
					)
				})?;
			let prev_mtime = self.cache.get_mtime(&local_path).unwrap_or(0);

			if !options.force_rebuild && prev_mtime >= mtime {
				output_message(
					ProcessQueueMessage::Skipped(&input),
					(1 + idx) as f64 / self.inputs.len() as f64,
				);
				continue;
			}
			output_message(
				ProcessQueueMessage::Parsing(&input),
				(1 + idx) as f64 / self.inputs.len() as f64,
			);

			// Create unit
			let source = Arc::new(SourceFile::new(input.clone(), None).unwrap());
			let unit = TranslationUnit::new(
				local_path.to_path_buf(),
				self.parser.clone(),
				source,
				false,
				true,
			);

			let output_file = match &output {
				ProcessOutputOptions::Directory(dir) => {
					let mut buf = dir.clone();
					let Some(mut basename) = local_path.file_stem().map(|str| str.to_os_string())
					else {
						Err(ProcessError::InputError(
							format!("Input file `{}` has no valid name!", local_path.display()),
							local_path.to_path_buf(),
						))?
					};
					basename.push(".html");
					buf.push(basename);
					buf
				}
				ProcessOutputOptions::File(file) => {
					let Some(mut basename) = local_path.file_stem().map(|str| str.to_os_string())
					else {
						Err(ProcessError::InputError(
							format!("Input file `{}` has no valid name!", local_path.display()),
							local_path.to_path_buf(),
						))?
					};
					basename.push(".html");
					PathBuf::from(basename)
				}
			};

			let (reports, unit) = unit.consume(output_file);
			Report::reports_to_stdout(unit.colors(), reports);
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
				for item in list {
					let mut path = self.project_path.clone();
					path.push(unit_file);
					let source = Arc::new(SourceFile::new(path, None).unwrap());
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
			return Err(ProcessError::LinkError(reports));
		}

		// Apply settings
		for unit in &mut processed {
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
				Ok(content) => {
					if let Some(output) = unit.output_path() {
						let mut parent = PathBuf::new();
						parent.push(output);
						parent.pop();
						let parent_exists = if let Ok(meta) = std::fs::metadata(&parent) {
							meta.is_dir()
						} else {
							false
						};
						if !parent_exists {
							if let Err(err) = std::fs::create_dir_all(&parent) {
								reports.push(make_err!(
									unit.token().source(),
									"Invalid output path".into(),
									span(
										unit.token().range,
										format!(
											"Failed to create output directory {}: {err}",
											parent.display().fg(Color::Blue),
										)
									)
								));
								break;
							}
						}
						if let Err(err) = std::fs::write(output, content) {
							reports.push(make_err!(
								unit.token().source(),
								"Invalid output path".into(),
								span(
									unit.token().range,
									format!(
										"Failed to output to {}: {err}",
										output.display().fg(Color::Blue),
									)
								)
							));
						}
					}
				}
				Err(err) => reports.extend(err),
			}
		}
		if !reports.is_empty() {
			return Err(ProcessError::CompileError(reports));
		}

		let time_now = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap()
			.as_secs();
		self.cache.export_units(processed.iter(), time_now);

		Ok(vec![])
	}
}
