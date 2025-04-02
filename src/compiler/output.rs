use std::collections::HashMap;
use std::future::Future;
use std::future::IntoFuture;
use std::pin::Pin;
use std::thread::sleep;
use std::time::Duration;

use tokio::task::JoinHandle;

use crate::parser::reports::Report;
use crate::parser::reports::ReportColors;

use super::compiler::Compiler;

pub struct CompilerOutput {
	// Holds the content of the resulting document
	pub(crate) content: String,
	/// Holds the spawned async tasks. After the work function has completed, these tasks will be waited on in order to insert their result in the compiled document
	tasks: Vec<(usize, JoinHandle<Result<String, Vec<Report>>>)>,
	/// The tasks runtime
	runtime: tokio::runtime::Runtime,
}

impl CompilerOutput {
	/// Run work function `f` with the task processor running
	///
	/// The result of async taks will be inserted into the output
	pub fn run_with_processor<F>(colors: &ReportColors, f: F) -> CompilerOutput
	where
		F: FnOnce(CompilerOutput) -> CompilerOutput,
	{
		// Create the output & the runtime
		let mut output = Self {
			content: String::default(),

			tasks: vec![],
			runtime: tokio::runtime::Builder::new_multi_thread()
				.worker_threads(8)
				.enable_all()
				.build()
				.unwrap(),
		};

		// Process the document with caller work-function
		output = f(output);

		// Wait for all tasks to finish [TODO Status message/Timer for tasks that may never finish]
		if !output.tasks.is_empty() {
			'outer: loop {
				for (_, handle) in &output.tasks {
					if !handle.is_finished() {
						break 'outer;
					}
				}
				sleep(Duration::from_millis(50));
			}
		}

		// Get results from async tasks
		let mut results = output.runtime.block_on(async {
			let mut results = vec![];
			for (pos, handle) in output.tasks.drain(..) {
				let result = handle.into_future().await.unwrap();
				results.push((pos, result));
			}
			output.tasks.clear();
			results
		});

		// Insert tasks results into output & offset references positions
		for (pos, result) in results.drain(..).rev() {
			match result {
				Ok(content) => {
					output.content.insert_str(pos, content.as_str()) 
				},
				Err(err) => todo!()/*Report::reports_to_stdout(colors, err)*/,
			}
		}
		output
	}

	/// Appends content to the output
	pub fn add_content<S: AsRef<str>>(&mut self, s: S) { self.content.push_str(s.as_ref()); }

	/// Adds an async task to the output. The task's result will be appended at the current output position
	///
	/// The task is a future that returns it's result in a string, or errors as a Vec of [`Report`]s
	pub fn add_task<F>(&mut self, task: Pin<Box<F>>)
	where
		F: Future<Output = Result<String, Vec<Report>>> + Send + 'static,
	{
		let handle = self.runtime.spawn(task);
		self.tasks.push((self.content.len(), handle));
	}

	pub fn content(
		&self,
	) -> &String {
		&self.content
	}
}
