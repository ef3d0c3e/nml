use std::{path::PathBuf, sync::Arc};

use crate::{cache::cache::Cache, parser::parser::Parser};

use super::compiler::{Compiler, Target};

/// Processqueue for inputs
pub struct ProcessQueue
{
	inputs: Vec<PathBuf>,
	cache: Arc<Cache>,

	parser: Parser,
	compiler: Compiler,
}

impl ProcessQueue {
	pub fn new(target: Target, db: Option<&str>, inputs: Vec<PathBuf>) -> Self
	{
		let cache = Arc::new(Cache::new(db)?);

		let parser = Parser::new();
		let compiler = Compiler::new(target, cache.clone());

		Self {
			inputs,
			cache,
			parser,
			compiler
		}
	}
}
