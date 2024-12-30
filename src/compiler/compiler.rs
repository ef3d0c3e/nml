use std::cell::Ref;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use rusqlite::Connection;

use crate::document::document::Document;
use crate::document::references::CrossReference;
use crate::document::references::ElemReference;
use crate::document::variable::Variable;

use super::postprocess::PostProcess;

#[derive(Clone, Copy)]
pub enum Target {
	HTML,
	#[allow(unused)]
	LATEX,
}

pub struct Compiler<'a> {
	target: Target,
	cache: Option<&'a Connection>,
	reference_count: RefCell<HashMap<String, HashMap<String, usize>>>,
	sections_counter: RefCell<Vec<usize>>,

	unresolved_references: RefCell<Vec<(usize, CrossReference)>>,
}

impl<'a> Compiler<'a> {
	pub fn new(target: Target, con: Option<&'a Connection>) -> Self {
		Self {
			target,
			cache: con,
			reference_count: RefCell::new(HashMap::new()),
			sections_counter: RefCell::new(vec![]),
			unresolved_references: RefCell::new(vec![]),
		}
	}

	/// Gets the section counter for a given depth
	/// This function modifies the section counter
	pub fn section_counter(&self, depth: usize) -> Ref<'_, Vec<usize>> {
		// Increment current counter
		if self.sections_counter.borrow().len() == depth {
			self.sections_counter
				.borrow_mut()
				.last_mut()
				.map(|id| *id += 1);
			return Ref::map(self.sections_counter.borrow(), |b| b);
		}

		// Close
		while self.sections_counter.borrow().len() > depth {
			self.sections_counter.borrow_mut().pop();
		}

		// Open
		while self.sections_counter.borrow().len() < depth {
			self.sections_counter.borrow_mut().push(1);
		}

		Ref::map(self.sections_counter.borrow(), |b| b)
	}

	/// Sanitizes text for a [`Target`]
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

	/// Sanitizes a format string for a [`Target`]
	///
	/// # Notes
	///
	/// This function may process invalid format string, which will be caught later
	/// by runtime_format.
	pub fn sanitize_format<S: AsRef<str>>(target: Target, str: S) -> String {
		match target {
			Target::HTML => {
				let mut out = String::new();

				let mut braces = 0;
				for c in str.as_ref().chars() {
					if c == '{' {
						out.push(c);
						braces += 1;
						continue;
					} else if c == '}' {
						out.push(c);
						if braces != 0 {
							braces -= 1;
						}
						continue;
					}
					// Inside format args
					if braces % 2 == 1 {
						out.push(c);
						continue;
					}

					match c {
						'&' => out += "&amp;",
						'<' => out += "&lt;",
						'>' => out += "&gt;",
						'"' => out += "&quot;",
						_ => out.push(c),
					}
				}

				out
			}
			_ => todo!("Sanitize not implemented"),
		}
	}

	/// Gets a reference name
	pub fn refname<S: AsRef<str>>(target: Target, str: S) -> String {
		Self::sanitize(target, str).replace(' ', "_")
	}

	/// Inserts or get a reference id for the compiled document
	///
	/// # Parameters
	/// - [`reference`] The reference to get or insert
	pub fn reference_id(&self, document: &dyn Document, reference: ElemReference) -> usize {
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

	/// Inserts a new crossreference
	pub fn insert_crossreference(&self, pos: usize, reference: CrossReference) {
		self.unresolved_references
			.borrow_mut()
			.push((pos, reference));
	}

	pub fn target(&self) -> Target { self.target }

	pub fn cache(&self) -> Option<&'a Connection> {
		self.cache
		//self.cache.as_ref().map(RefCell::borrow_mut)
	}

	pub fn header(&self, document: &dyn Document) -> String {
		pub fn get_variable_or_error(
			document: &dyn Document,
			var_name: &'static str,
		) -> Option<Rc<dyn Variable>> {
			document.get_variable(var_name).or_else(|| {
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
					result += format!(
						"<title>{}</title>",
						Compiler::sanitize(self.target(), page_title.to_string())
					)
					.as_str();
				}

				if let Some(css) = document.get_variable("html.css") {
					result += format!(
						"<link rel=\"stylesheet\" href=\"{}\">",
						Compiler::sanitize(self.target(), css.to_string())
					)
					.as_str();
				}
				result += r#"</head><body><div class="layout">"#;

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

	pub fn compile(&self, document: &dyn Document) -> (CompiledDocument, PostProcess) {
		let borrow = document.content().borrow();

		// Header
		let header = self.header(document);

		// Body
		let mut body = r#"<div class="content">"#.to_string();

		for i in 0..borrow.len() {
			let elem = &borrow[i];

			match elem.compile(self, document, body.len()) {
				Ok(result) => body.push_str(result.as_str()),
				Err(err) => println!("Unable to compile element: {err}\n{elem:#?}"),
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

		// References
		let references = document
			.scope()
			.borrow_mut()
			.referenceable
			.iter()
			.map(|(key, reference)| {
				let elem = document.get_from_reference(reference).unwrap();
				let refid = self.reference_id(document, *reference);

				(key.clone(), elem.refid(self, refid))
			})
			.collect::<HashMap<String, String>>();

		let postprocess = PostProcess {
			resolve_references: self.unresolved_references.replace(vec![]),
		};

		let cdoc = CompiledDocument {
			input: document.source().name().clone(),
			mtime: 0,
			variables,
			references,
			header,
			body,
			footer,
		};

		(cdoc, postprocess)
	}
}

#[derive(Debug)]
pub struct CompiledDocument {
	/// Input path relative to the input directory
	pub input: String,
	/// Modification time (i.e seconds since last epoch)
	pub mtime: u64,

	/// All the variables defined in the document
	/// with values mapped by [`Variable::to_string()`]
	pub variables: HashMap<String, String>,

	/// All the referenceable elements in the document
	/// with values mapped by [`crate::document::element::ReferenceableElement::refid()`]
	pub references: HashMap<String, String>,

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
		"CREATE TABLE IF NOT EXISTS compiled_documents(
			input TEXT PRIMARY KEY,
			mtime INTEGER NOT NULL,
			variables TEXT NOT NULL,
			internal_references TEXT NOT NULL,
			header TEXT NOT NULL,
			body TEXT NOT NULL,
			footer TEXT NOT NULL
		);"
	}

	fn sql_get_query() -> &'static str { "SELECT * FROM compiled_documents WHERE input = (?1)" }

	fn sql_insert_query() -> &'static str {
		"INSERT OR REPLACE INTO compiled_documents (input, mtime, variables, internal_references, header, body, footer) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
	}

	pub fn init_cache(con: &Connection) -> Result<usize, rusqlite::Error> {
		con.execute(Self::sql_table(), [])
	}

	pub fn from_cache(con: &Connection, input: &str) -> Option<Self> {
		con.query_row(Self::sql_get_query(), [input], |row| {
			Ok(CompiledDocument {
				input: input.to_string(),
				mtime: row.get_unwrap::<_, u64>(1),
				variables: serde_json::from_str(row.get_unwrap::<_, String>(2).as_str()).unwrap(),
				references: serde_json::from_str(row.get_unwrap::<_, String>(3).as_str()).unwrap(),
				header: row.get_unwrap::<_, String>(4),
				body: row.get_unwrap::<_, String>(5),
				footer: row.get_unwrap::<_, String>(6),
			})
		})
		.ok()
	}

	/// Interts [`CompiledDocument`] into cache
	pub fn insert_cache(&self, con: &Connection) -> Result<usize, rusqlite::Error> {
		con.execute(
			Self::sql_insert_query(),
			(
				&self.input,
				&self.mtime,
				serde_json::to_string(&self.variables).unwrap(),
				serde_json::to_string(&self.references).unwrap(),
				&self.header,
				&self.body,
				&self.footer,
			),
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn sanitize_test() {
		assert_eq!(Compiler::sanitize(Target::HTML, "<a>"), "&lt;a&gt;");
		assert_eq!(Compiler::sanitize(Target::HTML, "&lt;"), "&amp;lt;");
		assert_eq!(Compiler::sanitize(Target::HTML, "\""), "&quot;");

		assert_eq!(
			Compiler::sanitize_format(Target::HTML, "{<>&\"}"),
			"{<>&\"}"
		);
		assert_eq!(
			Compiler::sanitize_format(Target::HTML, "{{<>}}"),
			"{{&lt;&gt;}}"
		);
		assert_eq!(Compiler::sanitize_format(Target::HTML, "{{<"), "{{&lt;");
	}
}
