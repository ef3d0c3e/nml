use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
use std::future::IntoFuture;
use std::hash::Hasher;
use std::hash::Hash;
use std::pin::Pin;
use std::rc::Rc;
use std::thread::sleep;
use std::time::Duration;

use tokio::task::JoinHandle;

use crate::parser::reports::Report;
use crate::parser::reports::ReportColors;
use crate::unit::references::Refname;
use crate::unit::scope::Scope;

use super::compiler::Compiler;
use super::compiler::Target;

#[derive(Debug)]
struct RcKey<T>(Rc<RefCell<T>>);

impl<T> PartialEq for RcKey<T> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0) // Compare Rc pointer addresses
    }
}

impl<T> Eq for RcKey<T> {}

impl<T> Hash for RcKey<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_usize(Rc::as_ptr(&self.0) as usize); // Hash the pointer address
    }
}

pub struct CompilerOutput {
	/// Compilation target
	target: Target,
	/// Paragraph state of the output
	paragraph: HashSet<RcKey<Scope>>,
	/// Internal links
	internal_links: HashMap<String, String>,

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
	pub fn run_with_processor<F>(target: Target, colors: &ReportColors, f: F) -> CompilerOutput
	where
		F: FnOnce(CompilerOutput) -> CompilerOutput,
	{
		// Create the output & the runtime
		let mut output = Self {
			target,
			paragraph: HashSet::new(),
			internal_links: HashMap::new(),
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

	pub fn in_paragraph(&mut self, scope: &Rc<RefCell<Scope>>) -> bool {
		self.paragraph.contains(&RcKey(scope.to_owned()))
	}

	pub fn set_paragraph(&mut self, scope: &Rc<RefCell<Scope>>, value: bool) {
		if value
		{
			self.paragraph.insert(RcKey(scope.to_owned()));
		}
		else
		{
			self.paragraph.remove(&RcKey(scope.to_owned()));
		}
	}

	/// Gets an internal link name for a given refname
	/// The given refname has to be an internal refname
	pub fn get_link(&mut self, refname: &Refname) -> String
	{
		let Refname::Internal(name) = refname else { panic!("Expected internal refname") };
		if let Some(internal) = self.internal_links.get(name)
		{
			internal.to_owned()
		}
		else
		{
			match self.target {
				Target::HTML => {
					let mut transformed = name.chars()
						.map(|c| {
							if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'
							{
								c
							}
							else if c == ' '
							{
								'-'
							}
							else
							{
								'_'
							}
						}).collect::<String>();
					while self.internal_links.iter().find(|(_, value)| **value == transformed).is_some()
					{
						transformed.push('_');
					}
					self.internal_links.insert(name.to_owned(), transformed);
				},
				Target::LATEX => todo!(),
			}
			self.internal_links.get(name).unwrap().to_owned()
		}
	}
}
