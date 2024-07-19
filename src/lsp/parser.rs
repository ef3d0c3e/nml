use std::{cell::RefCell, collections::HashMap};

use crate::{elements::registrar::register, lua::kernel::Kernel, parser::{rule::Rule, state::StateHolder}};

struct LSParser
{
	rules: Vec<Box<dyn Rule>>,

	// Parser state
	pub state: RefCell<StateHolder>,
	//pub kernels: RefCell<HashMap<String, Kernel>>,
}

impl LSParser {
	pub fn default() -> Self
	{
		let mut parser = LSParser {
			rules: vec![],
			state: RefCell::new(StateHolder::new()),
			//kernels: RefCell::new(HashMap::new()),
		};

		// TODO: Main kernel
		//register(&mut parser);

		parser
	}
}


