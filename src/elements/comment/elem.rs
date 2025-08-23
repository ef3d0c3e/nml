use std::sync::Arc;

use auto_userdata::AutoUserData;
use parking_lot::RwLock;

use crate::{compiler::{compiler::Compiler, output::CompilerOutput}, parser::{reports::Report, source::Token}, unit::{element::{ElemKind::{self, Invisible}, Element}, scope::Scope}};

#[derive(Debug, AutoUserData)]
pub struct Comment {
	pub(crate) location: Token,
	pub(crate) content: String,
}

impl Element for Comment {
    fn location(&self) -> &Token {
        &self.location
    }

    fn kind(&self) -> ElemKind {
        Invisible
    }

    fn element_name(&self) -> &'static str {
        "Comment"
    }

    fn compile(
		    &self,
		    _scope: Arc<RwLock<Scope>>,
		    _compiler: &Compiler,
		    _output: &mut CompilerOutput,
	    ) -> Result<(), Vec<Report>> {
		Ok(())
    }
}
