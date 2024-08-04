use std::{cell::{Ref, RefCell, RefMut}, collections::HashMap, rc::Rc};

use crate::{document::{customstyle::{CustomStyle, CustomStyleHolder}, document::Document, element::Element, layout::{LayoutHolder, LayoutType}, style::{ElementStyle, StyleHolder}}, lua::kernel::{Kernel, KernelHolder}, parser::{parser::{Parser, ReportColors}, rule::Rule, source::{Cursor, Source}, state::StateHolder}};

#[derive(Debug, Clone)]
pub struct LineCursor
{
	pub pos: usize,
	pub line: usize,
	pub line_pos: usize,
	pub source: Rc<dyn Source>,
}

impl LineCursor
{
	/// Creates [`LineCursor`] at position
	///
	/// # Error
	/// This function will panic if [`pos`] is not utf8 aligned
	///
	/// Note: this is a convenience function, it should be used
	/// with parsimony as it is expensive
	pub fn at(&mut self, pos: usize)
	{
		if pos > self.pos
		{
			let start = self.pos;
			//eprintln!("slice{{{}}}, want={pos}", &self.source.content().as_str()[start..pos]);
			let mut it = self.source.content()
				.as_str()[start..] // pos+1
				.chars()
				.peekable();

			let mut prev = self.source.content()
					.as_str()[..start+1]
					.chars()
					.rev()
					.next();
			//eprintln!("prev={prev:#?}");
			while self.pos < pos
			{
				let c = it.next().unwrap();
				let len = c.len_utf8();

				self.pos += len;
				if prev == Some('\n')
				{
					self.line += 1;
					self.line_pos = 0;
				}
				else
				{
					self.line_pos += len;
				}

				//eprintln!("({}, {c:#?}) ({} {})", self.pos, self.line, self.line_pos);
				prev = Some(c);
			}
		}
		else if pos < self.pos
		{
			todo!("");
			self.source.content()
				.as_str()[pos..self.pos]
				.char_indices()
				.rev()
				.for_each(|(len, c)| {
					self.pos -= len;
					if c == '\n'
					{
						self.line -= 1;
					}
				});
			self.line_pos = self.source.content()
				.as_str()[..self.pos]
				.char_indices()
				.rev()
				.find(|(_, c)| *c == '\n')
				.map(|(line_start, _)| self.pos-line_start)
				.unwrap_or(0);
		}

		// May fail if pos is not utf8-aligned
		assert_eq!(pos, self.pos);
	}
}

impl From<&LineCursor> for Cursor
{
    fn from(value: &LineCursor) -> Self {
		Self {
			pos: value.pos,
			source: value.source.clone()
		}
    }
}

#[derive(Debug)]
pub struct LsParser
{
	rules: Vec<Box<dyn Rule>>,
	colors: ReportColors,

	// Parser state
	pub state: RefCell<StateHolder>,
	pub kernels: RefCell<HashMap<String, Kernel>>,
}

impl Parser for LsParser
{
    fn colors(&self) -> &ReportColors { &self.colors }
    fn rules(&self) -> &Vec<Box<dyn Rule>> { &self.rules }
    fn rules_mut(&mut self) -> &mut Vec<Box<dyn Rule>> { &mut self.rules }

	fn state(&self) -> Ref<'_, StateHolder> { self.state.borrow() }
	fn state_mut(&self) -> std::cell::RefMut<'_, StateHolder> { self.state.borrow_mut() }
	
	fn has_error(&self) -> bool { true }

    fn push<'a>(&self, doc: &dyn Document, elem: Box<dyn Element>) {
        todo!()
    }

    fn parse<'a>(&self, source: Rc<dyn Source>, parent: Option<&'a dyn Document<'a>>) -> Box<dyn Document<'a>+'a> {
        todo!()
    }

    fn parse_into<'a>(&self, source: Rc<dyn Source>, document: &'a dyn Document<'a>) {
        todo!()
    }
}

impl KernelHolder for LsParser
{
    fn get_kernel(&self, name: &str)
		-> Option<RefMut<'_, Kernel>> {
		RefMut::filter_map(self.kernels.borrow_mut(),
		|map| map.get_mut(name)).ok()
    }

    fn insert_kernel(&self, name: String, kernel: Kernel)
		-> RefMut<'_, Kernel> {
			//TODO do not get
		self.kernels.borrow_mut()
			.insert(name.clone(), kernel);
		self.get_kernel(name.as_str()).unwrap()
    }
}

impl StyleHolder for LsParser {
    fn element_styles(&self) -> Ref<'_, HashMap<String, Rc<dyn ElementStyle>>> {
        todo!()
    }

    fn element_styles_mut(&self) -> RefMut<'_, HashMap<String, Rc<dyn ElementStyle>>> {
        todo!()
    }
}

impl LayoutHolder for LsParser {
    fn layouts(&self) -> Ref<'_, HashMap<String, Rc<dyn LayoutType>>> {
        todo!()
    }

    fn layouts_mut(&self) -> RefMut<'_, HashMap<String, Rc<dyn LayoutType>>> {
        todo!()
    }
}

impl CustomStyleHolder for LsParser {
    fn custom_styles(&self) -> Ref<'_, HashMap<String, Rc<dyn CustomStyle>>> {
        todo!()
    }

    fn custom_styles_mut(&self) -> RefMut<'_, HashMap<String, Rc<dyn CustomStyle>>> {
        todo!()
    }
}
