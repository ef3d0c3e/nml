use std::cell::RefCell;
use std::cell::RefMut;
use std::collections::HashMap;
use std::rc::Rc;

use rusqlite::Connection;

use crate::document::document::Document;
use crate::document::document::ElemReference;
use crate::document::variable::Variable;

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

	pub fn sanitize<S: AsRef<str>>(target: Target, str: S) -> String {
		match target {
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
					result += format!("<title>{}</title>", Compiler::sanitize(self.target(), page_title.to_string()))
						.as_str();
				}

				if let Some(css) = document.get_variable("html.css") {
					result += format!(
						"<link rel=\"stylesheet\" href=\"{}\">",
						Compiler::sanitize(self.target(), css.to_string())
					)
					.as_str();
				}
				result += r#"</head><body><div id="layout">"#;

				// TODO: TOC
				// TODO: Author, Date, Title, Div
			}
			Target::LATEX => {}
		}
		result
	}

	pub fn footer(&self, _document: &dyn Document) -> String {
		let mut result = String::new();
		match self.target() {
			Target::HTML => {
				result += "</div></body></html>";
			}
			Target::LATEX => todo!(""),
		}
		result
	}

	pub fn compile(&self, document: &dyn Document) -> CompiledDocument {
		let borrow = document.content().borrow();

		// Header
		let header = self.header(document);

		// Body
		let mut body = r#"<div id="content">"#.to_string();
		for i in 0..borrow.len() {
			let elem = &borrow[i];

			match elem.compile(self, document) {
				Ok(result) => body.push_str(result.as_str()),
				Err(err) => println!("Unable to compile element: {err}\n{}", elem.to_string()),
			}
		}
		body.push_str("</div>");

		// Footer
		let footer = self.footer(document);

		// Variables
		let variables = document
			.scope()
			.borrow_mut()
			.variables
			.iter()
			.map(|(key, var)| (key.clone(), var.to_string()))
			.collect::<HashMap<String, String>>();

		CompiledDocument {
			input: document.source().name().clone(),
			mtime: 0,
			variables,
			header,
			body,
			footer,
		}
	}
}

#[derive(Debug)]
pub struct CompiledDocument {
	/// Input path relative to the input directory
	pub input: String,
	/// Modification time (i.e seconds since last epoch)
	pub mtime: u64,

	// TODO: Also store exported references
	// so they can be referenced from elsewhere
	// This will also require rebuilding in case some exported references have changed...
	/// Variables exported to string, so they can be querried later
	pub variables: HashMap<String, String>,

	/// Compiled document's header
	pub header: String,
	/// Compiled document's body
	pub body: String,
	/// Compiled document's footer
	pub footer: String,
}

impl CompiledDocument {
	pub fn get_variable(&self, name: &str) -> Option<&String> { self.variables.get(name) }

	fn sql_table() -> &'static str {
		"CREATE TABLE IF NOT EXISTS compiled_documents (
			input TEXT PRIMARY KEY,
			mtime INTEGER NOT NULL,
			variables TEXT NOT NULL,
			header TEXT NOT NULL,
			body TEXT NOT NULL,
			footer TEXT NOT NULL
		);"
	}

	fn sql_get_query() -> &'static str { "SELECT * FROM compiled_documents WHERE input = (?1)" }

	fn sql_insert_query() -> &'static str {
		"INSERT OR REPLACE INTO compiled_documents (input, mtime, variables, header, body, footer) VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
	}

	pub fn init_cache(con: &Connection) -> Result<usize, rusqlite::Error> {
		con.execute(Self::sql_table(), [])
	}

	pub fn from_cache(con: &Connection, input: &String) -> Option<Self> {
		con.query_row(Self::sql_get_query(), [input], |row| {
			Ok(CompiledDocument {
				input: input.clone(),
				mtime: row.get_unwrap::<_, u64>(1),
				variables: serde_json::from_str(row.get_unwrap::<_, String>(2).as_str()).unwrap(),
				header: row.get_unwrap::<_, String>(3),
				body: row.get_unwrap::<_, String>(4),
				footer: row.get_unwrap::<_, String>(5),
			})
		})
		.ok()
	}

	/// Inserts [`CompiledDocument`] into cache
	pub fn insert_cache(&self, con: &Connection) -> Result<usize, rusqlite::Error> {
		con.execute(
			Self::sql_insert_query(),
			(
				&self.input,
				&self.mtime,
				serde_json::to_string(&self.variables).unwrap(),
				&self.header,
				&self.body,
				&self.footer,
			),
		)
	}
}
