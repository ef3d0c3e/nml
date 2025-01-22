use std::collections::HashMap;
use std::future::Future;
use std::future::IntoFuture;
use std::pin::Pin;
use std::thread::sleep;
use std::time::Duration;

use tokio::task::JoinHandle;

use crate::document::document::Document;
use crate::document::references::CrossReference;
use crate::document::references::ElemReference;
use crate::parser::parser::ReportColors;
use crate::parser::reports::Report;

use super::compiler::CompiledDocument;
use super::compiler::Compiler;
use super::postprocess::PostProcess;

pub struct CompilerOutput {
	// Holds the content of the resulting document
	pub(crate) content: String,
	/// Holds the position of every cross-document reference
	references: Vec<(usize, CrossReference)>,
	/// Holds the current id used for special references that have a `refcount_key`
	special_references: HashMap<&'static str, HashMap<String, usize>>,
	/// Holds the id of each sections until a certain depth
	/// It works similarly as a stack where only the last value can be incremented, or the entire stack can be popped/pushed into
	sections_counter: Vec<usize>,

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
			references: Vec::default(),
			special_references: HashMap::default(),
			sections_counter: Vec::default(),

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
					// Offset references positions
					output.references
						.iter_mut()
						.for_each(|(rpos, _)| if *rpos >= pos { *rpos += content.len() });
					output.content.insert_str(pos, content.as_str()) 
				},
				Err(err) => Report::reports_to_stdout(colors, err),
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

	/// Inserts a new cross-reference that will be resolved during post-processing.
	///
	/// Once resolved, a link to the references element will be inserted at the current output position.
	///
	/// # Note
	///
	/// There can only be one cross-reference at a given output position.
	/// In case another cross-reference is inserted at the same location (which should never happen),
	/// The program will panic
	pub fn add_external_reference(&mut self, xref: CrossReference) {
		if self.references.get(self.content.len()).is_some() {
			panic!("Duplicate cross-reference in one location");
		}
		self.references.push((self.content.len(), xref));
	}

	/// Inserts or get a reference id for the compiled document
	///
	/// The returned index corresponds either to the index of the previous reference with the same refname, or the index of the newly inserted reference.
	///
	/// # Parameters
	/// - [`reference`] The reference to get or insert
	pub fn reference_id(&mut self, document: &dyn Document, reference: ElemReference) -> usize {
		let reference = document.get_from_reference(&reference).unwrap();
		let refkey = reference.refcount_key();
		let refname = reference.reference_name().unwrap();

		let map = match self.special_references.get_mut(refkey) {
			Some(map) => map,
			None => {
				self.special_references.insert(refkey, HashMap::new());
				self.special_references.get_mut(refkey).unwrap()
			}
		};

		if let Some(elem) = map.get(refname) {
			// Return already existing ref
			*elem
		} else {
			// Insert new ref
			let index = map
				.iter()
				.fold(0, |max, (_, value)| std::cmp::max(max, *value));
			map.insert(refname.clone(), index + 1);
			index + 1
		}
	}

	/// Gets the section counter for a given depth
	/// This function modifies the section counter
	pub fn next_section_counter(&mut self, depth: usize) -> &Vec<usize> {
		// Increment current counter
		if self.sections_counter.len() == depth {
			self.sections_counter.last_mut().map(|id| *id += 1);
			return &self.sections_counter;
		}

		// Close sections
		while self.sections_counter.len() > depth {
			self.sections_counter.pop();
		}

		// Open new sections
		while self.sections_counter.len() < depth {
			self.sections_counter.push(1);
		}

		return &self.sections_counter;
	}

	pub fn to_compiled(
		mut self,
		compiler: &Compiler,
		document: &dyn Document,
		header: String,
		footer: String,
	) -> (CompiledDocument, PostProcess) {
		// Variables
		let variables = document
			.scope()
			.borrow_mut()
			.variables
			.iter()
			.map(|(key, var)| (key.clone(), var.to_string()))
			.collect::<HashMap<String, String>>();

		// References
		//let references = document
		//	.scope()
		//	.borrow_mut()
		//	.referenceable
		//	.iter()
		//	.map(|(key, reference)| {
		//		let elem = document.get_from_reference(reference).unwrap();
		//		let refid = self.reference_id(document, *reference);

		//		(key.clone(), elem.refid(compiler, refid))
		//	})
		//	.collect::<HashMap<String, String>>();

		let postprocess = PostProcess {
			resolve_references: vec![], //self.unresolved_references.replace(vec![]),
		};

		let cdoc = CompiledDocument {
			input: document.source().name().clone(),
			mtime: 0,
			variables,
			references: HashMap::default(),
			header,
			body: self.content,
			footer,
		};

		(cdoc, postprocess)
	}
}
