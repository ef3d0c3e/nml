use std::{cell::{RefCell, RefMut}, rc::Rc};


use rusqlite::Connection;

use crate::document::{document::Document, variable::Variable};

#[derive(Clone, Copy)]
pub enum Target
{
	HTML,
	LATEX,
}

pub struct Compiler
{
	target: Target,
	cache: Option<RefCell<Connection>>,
}

impl Compiler
{
	pub fn new(target: Target, db_path: Option<String>) -> Self {
		let cache = match db_path
		{
			None => None,
			Some(path) => {
				match Connection::open(path)
				{
					Err(e) => panic!("Cannot connect to database: {e}"),
					Ok(con) => Some(con),
				}
			}
		};
		Self {
			target,
			cache: cache.map(|con| RefCell::new(con)),
		}
	}

	pub fn target(&self) -> Target
	{
		self.target
	}

	pub fn cache(&self) -> Option<RefMut<'_, Connection>>
	{
		self.cache
			.as_ref()
			.map(RefCell::borrow_mut)
	}

	pub fn sanitize<S: AsRef<str>>(&self, str: S) -> String {
		match self.target
		{
			Target::HTML => str.as_ref()
				.replace("&", "&amp;")
				.replace("<", "&lt;")
				.replace(">", "&gt;")
				.replace("\"", "&quot;"),
			_ => todo!("Sanitize not implemented")
		}
	}

	pub fn header(&self, document: &Document) -> String
	{
		pub fn get_variable_or_error(document: &Document, var_name: &'static str) -> Option<Rc<dyn Variable>>
		{
			document.get_variable(var_name)
				.and_then(|(_, var)| Some(var))
				.or_else(|| {
					println!("Missing variable `{var_name}` in {}", document.source().name());
					None
				})
		}

		let mut result = String::new();
		match self.target()
		{
			Target::HTML => {
				result += "<!DOCTYPE HTML><html><head>";
				result += "<meta charset=\"UTF-8\">";
				if let Some(page_title) = get_variable_or_error(document, "html.page_title")
				{
					result += format!("<title>{}</title>", self.sanitize(page_title.to_string())).as_str();
				}

				if let Some((_, css)) = document.get_variable("html.css")
				{
					result += format!("<link rel=\"stylesheet\" href=\"{}\">", self.sanitize(css.to_string())).as_str();
				}
				result += "</head><body>";

				// TODO: TOC
				// TODO: Author, Date, Title, Div
			},
			Target::LATEX => {

			},
		}
		result
	}

	pub fn footer(&self, _document: &Document) -> String
	{
		let mut result = String::new();
		match self.target()
		{
			Target::HTML => {
				result += "</body></html>";
			},
			Target::LATEX => {

			},
		}
		result
	}
	
	pub fn compile(&self, document: &Document) -> String
	{
		let mut out = String::new();
		let borrow = document.content.borrow();

		// Header
		out += self.header(document).as_str();
		
		// Body
		for i in 0 .. borrow.len()
		{
			let elem = &borrow[i];
			//let prev = match i
			//{
			//	0 => None,
			//	_ => borrow.get(i-1),
			//};
			//let next = borrow.get(i+1);

			match elem.compile(self, document)
			{
				Ok(result) => {
					//println!("Elem: {}\nCompiled to: {result}", elem.to_string());
					out.push_str(result.as_str())
				},
				Err(err) => println!("Unable to compile element: {err}\n{}", elem.to_string())
			}
		}

		// Footer
		out += self.footer(document).as_str();

		out
	}
}
