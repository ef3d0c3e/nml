use std::cell::RefCell;
use std::cell::RefMut;
use std::collections::HashMap;
use std::rc::Rc;

use rusqlite::Connection;

use crate::document::document::Document;
use crate::document::document::ElemReference;
use crate::document::variable::Variable;

use super::navigation::NavEntry;
use super::navigation::Navigation;

#[derive(Clone, Copy)]
pub enum Target {
	HTML,
	LATEX,
}

pub struct Compiler {
	target: Target,
	cache: Option<RefCell<Connection>>,
	reference_count: RefCell<HashMap<String, HashMap<String, usize>>>,
	// TODO: External references, i.e resolved later
}

impl Compiler {
	pub fn new(target: Target, db_path: Option<String>) -> Self {
		let cache = match db_path {
			None => None,
			Some(path) => match Connection::open(path) {
				Err(e) => panic!("Cannot connect to database: {e}"),
				Ok(con) => Some(con),
			},
		};
		Self {
			target,
			cache: cache.map(|con| RefCell::new(con)),
			reference_count: RefCell::new(HashMap::new()),
		}
	}

	/// Inserts or get a reference id for the compiled document
	///
	/// # Parameters
	/// - [`reference`] The reference to get or insert
	pub fn reference_id<'a>(&self, document: &'a dyn Document, reference: ElemReference) -> usize {
		let mut borrow = self.reference_count.borrow_mut();
		let reference = document.get_from_reference(&reference).unwrap();
		let refkey = reference.refcount_key();
		let refname = reference.reference_name().unwrap();

		let map = match borrow.get_mut(refkey) {
			Some(map) => map,
			None => {
				borrow.insert(refkey.to_string(), HashMap::new());
				borrow.get_mut(refkey).unwrap()
			}
		};

		if let Some(elem) = map.get(refname) {
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

	pub fn target(&self) -> Target { self.target }

	pub fn cache(&self) -> Option<RefMut<'_, Connection>> {
		self.cache.as_ref().map(RefCell::borrow_mut)
	}

	pub fn sanitize<S: AsRef<str>>(&self, str: S) -> String {
		match self.target {
			Target::HTML => str
				.as_ref()
				.replace("&", "&amp;")
				.replace("<", "&lt;")
				.replace(">", "&gt;")
				.replace("\"", "&quot;"),
			_ => todo!("Sanitize not implemented"),
		}
	}

	pub fn header(&self, document: &dyn Document) -> String {
		pub fn get_variable_or_error(
			document: &dyn Document,
			var_name: &'static str,
		) -> Option<Rc<dyn Variable>> {
			document
				.get_variable(var_name)
				.and_then(|var| Some(var))
				.or_else(|| {
					println!(
						"Missing variable `{var_name}` in {}",
						document.source().name()
					);
					None
				})
		}

		let mut result = String::new();
		match self.target() {
			Target::HTML => {
				result += "<!DOCTYPE HTML><html><head>";
				result += "<meta charset=\"UTF-8\">";
				if let Some(page_title) = get_variable_or_error(document, "html.page_title") {
					result += format!("<title>{}</title>", self.sanitize(page_title.to_string()))
						.as_str();
				}

				if let Some(css) = document.get_variable("html.css") {
					result += format!(
						"<link rel=\"stylesheet\" href=\"{}\">",
						self.sanitize(css.to_string())
					)
					.as_str();
				}
				result += "</head><body>";

				// TODO: TOC
				// TODO: Author, Date, Title, Div
			}
			Target::LATEX => {}
		}
		result
	}

	pub fn navigation(&self, navigation: &Navigation, document: &dyn Document) -> String
	{
		let mut result = String::new();
		match self.target()
		{
			Target::HTML => {
				result += r#"<ul id="navbar">"#;

				fn process(result: &mut String, name: &String, ent: &NavEntry, depth: usize)
				{
					let ent_path = ent.path.as_ref()
						.map_or("#".to_string(),|path| path.clone());
					result.push_str(format!(r#"<li><a href="{ent_path}">{name}</a></li>"#).as_str());

					if let Some(children) = ent.children.as_ref()
					{
						result.push_str("<ul>");
						for (name, ent) in children
						{
							process(result, name, ent, depth+1);
						}
						result.push_str("</ul>");
					}
				}

				for (name, ent) in &navigation.entries
				{
					process(&mut result, name, ent, 0);
				}
				

				result += r#"</ul>"#;
			},
			_ => todo!("")
		}
		result
	}

	pub fn footer(&self, _document: &dyn Document) -> String {
		let mut result = String::new();
		match self.target() {
			Target::HTML => {
				result += "</body></html>";
			}
			Target::LATEX => todo!("")
		}
		result
	}

	pub fn compile(&self, navigation: &Navigation, document: &dyn Document) -> String {
		let mut out = String::new();
		let borrow = document.content().borrow();

		// Header
		out += self.header(document).as_str();

		// Navigation
		out += self.navigation(navigation, document).as_str();

		// Body
		for i in 0..borrow.len() {
			let elem = &borrow[i];

			match elem.compile(self, document) {
				Ok(result) => {
					out.push_str(result.as_str())
				}
				Err(err) => println!("Unable to compile element: {err}\n{}", elem.to_string()),
			}
		}

		// Footer
		out += self.footer(document).as_str();

		out
	}
}
