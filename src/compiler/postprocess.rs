use std::ops::Range;

use crate::unit::{references::Refname, unit::Reference};

/// Task that runs after documents have been compiled
pub trait PostProcessTask
{
	/// Units required to finish compiling before this task may complete
	fn requirements(&self) -> &Vec<String>;
}

pub struct ResolveLinkTask
{
	/// Required units
	requirements: Vec<String>,
	/// Target file
	target: String,
	/// Position to insert resolved link at
	pos: usize,
}

impl ResolveLinkTask
{
	pub fn new(reference: Reference, pos: usize) -> Self
	{
		Self {
			requirements: vec![reference.source_unit.clone()],
			target: todo!(),
			pos,
		}
	}
}

impl PostProcessTask for ResolveLinkTask
{
    fn requirements(&self) -> &Vec<String> {
        &self.requirements
    }
}
