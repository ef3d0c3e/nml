use std::cell::OnceCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::future::IntoFuture;
use std::hash::Hash;
use std::hash::Hasher;
use std::pin::Pin;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::unit::element::ReferenceableElement;

use ariadne::Color;
use ariadne::Fmt;
use parking_lot::RwLock;
use tokio::task::JoinHandle;

use crate::make_err;
use crate::parser::reports::Report;
use crate::parser::reports::ReportColors;
use crate::parser::source::Token;
use crate::unit::scope::Scope;

use super::compiler::Target;

#[derive(Debug)]
struct ArcKey<T>(Arc<RwLock<T>>);

impl<T> PartialEq for ArcKey<T> {
	fn eq(&self, other: &Self) -> bool {
		Arc::ptr_eq(&self.0, &other.0) // Compare Rc pointer addresses
	}
}

impl<T> Eq for ArcKey<T> {}

impl<T> Hash for ArcKey<T> {
	fn hash<H: Hasher>(&self, state: &mut H) {
		state.write_usize(Arc::as_ptr(&self.0) as usize); // Hash the pointer address
	}
}

/// Async task processed by the output
#[derive(Debug)]
pub struct OutputTask {
	/// Task handle
	handle: OnceCell<JoinHandle<Result<String, Vec<Report>>>>,
	/// Task location in it's original scope
	location: Token,
	/// Task position in the final output
	pos: usize,
	/// Task name
	name: String,
	/// Task timeout in milliseconds
	timeout: u128,
	/// Task result
	result: OnceCell<Result<String, Vec<Report>>>,
}

pub struct CompilerOutput {
	/// Compilation target
	target: Target,
	/// Paragraph state of the output
	paragraph: HashSet<ArcKey<Scope>>,
	/// Counter for references
	refcount: HashMap<String, usize>,

	// Holds the content of the resulting document
	pub(crate) content: String,
	/// Holds the spawned async tasks. After the work function has completed, these tasks will be
	/// waited on in order to insert their result in the compiled document
	tasks: Vec<OutputTask>,
	/// The tasks runtime
	runtime: tokio::runtime::Runtime,
}

impl CompilerOutput {
	/// Run work function `f` with the task processor running
	///
	/// The result of async taks will be inserted into the output
	pub fn run_with_processor<F>(
		target: Target,
		colors: &ReportColors,
		f: F,
	) -> Result<CompilerOutput, Vec<Report>>
	where
		F: FnOnce(CompilerOutput) -> CompilerOutput,
	{
		// Create the output & the runtime
		let mut output = Self {
			target,
			paragraph: HashSet::default(),
			refcount: HashMap::default(),

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

		// Wait for all tasks to finish
		let mut finished = 0;
		let time_start = Instant::now();
		while finished != output.tasks.len() {
			for task in &mut output.tasks {
				if task.result.get().is_some() || !task.handle.get().is_some() {
					continue;
				}

				if task
					.handle
					.get()
					.map_or(false, |handle| handle.is_finished())
				{
					output
						.runtime
						.block_on(async {
							task.result
								.set(task.handle.take().unwrap().into_future().await.unwrap())
						})
						.unwrap();
					task.handle.take();
					finished += 1;
					continue;
				} else if time_start.elapsed().as_millis() < task.timeout {
					continue;
				}

				task.handle.get().unwrap().abort();
				task.handle.take();
				println!("Aborted task `{}`, timeout exceeded", task.name);
				finished += 1;
			}
			println!(
				"[{}/{}] Waiting for tasks... ({}ms)",
				finished,
				output.tasks.len(),
				time_start.elapsed().as_millis()
			);
			sleep(Duration::from_millis(500));
		}

		// Check for errors
		let mut reports = vec![];
		for task in &mut output.tasks {
			if task.result.get().is_some_and(Result::is_ok) {
				continue;
			}

			if task.result.get().is_none() {
				reports.push(make_err!(
					task.location.source(),
					"Task processing failed".into(),
					span(
						task.location.range.clone(),
						format!(
							"Processing for task `{}` timed out",
							(&task.name).fg(Color::Green),
						)
					)
				));
				continue;
			}
			let Some(Err(mut err)) = task.result.take() else {
				panic!()
			};
			reports.extend(err.drain(..));
		}
		if !reports.is_empty() {
			return Err(reports);
		}

		// Insert tasks results into output & offset references positions
		for (pos, content) in output
			.tasks
			.iter()
			.rev()
			.map(|task| (task.pos, task.result.get().unwrap().as_ref().unwrap()))
		{
			output.content.insert_str(pos, content.as_str());
		}
		Ok(output)
	}

	/// Appends content to the output
	pub fn add_content<S: AsRef<str>>(&mut self, s: S) {
		self.content.push_str(s.as_ref());
	}

	/// Adds an async task to the output. The task's result will be appended at the current output position
	///
	/// The task is a future that returns it's result in a string, or errors as a Vec of [`Report`]s
	pub fn add_task<F>(&mut self, location: Token, name: String, task: Pin<Box<F>>)
	where
		F: Future<Output = Result<String, Vec<Report>>> + Send + 'static,
	{
		let handle = self.runtime.spawn(task);
		self.tasks.push(OutputTask {
			handle: OnceCell::from(handle),
			location,
			pos: self.content.len(),
			name,
			timeout: 5000,
			result: OnceCell::default(),
		});
	}

	pub fn content(&self) -> &String {
		&self.content
	}

	pub fn in_paragraph(&mut self, scope: &Arc<RwLock<Scope>>) -> bool {
		self.paragraph.contains(&ArcKey(scope.to_owned()))
	}

	pub fn set_paragraph(&mut self, scope: &Arc<RwLock<Scope>>, value: bool) {
		if value {
			self.paragraph.insert(ArcKey(scope.to_owned()));
		} else {
			self.paragraph.remove(&ArcKey(scope.to_owned()));
		}
	}

	/// Get a unique reference id for the element's referenceable type
	pub fn refid(&mut self, refer: &dyn ReferenceableElement) -> usize {
		let key = refer.refcount_key();
		if let Some(count) = self.refcount.get_mut(key)
		{
			*count += 1;
			*count
		}
		else
		{
			self.refcount.insert(key.to_owned(), 1);
			1
		}
	}
}
