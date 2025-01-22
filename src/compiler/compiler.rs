use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use crate::cache::cache::Cache;
use crate::document::document::Document;
use crate::document::variable::Variable;
use crate::parser::parser::ReportColors;
use crate::parser::reports::Report;
use rusqlite::Connection;
use tokio::sync::MutexGuard;

use super::output::CompilerOutput;
use super::postprocess::PostProcess;
use super::sanitize::Sanitizer;

#[derive(Clone, Copy)]
pub enum Target {
	HTML,
	#[allow(unused)]
	LATEX,
}

// TODO: Compiler should be immutable
pub struct Compiler {
	target: Target,
	cache: Arc<Cache>,
	sanitizer: Sanitizer,
}

impl Compiler {
	pub fn new(target: Target, cache: Arc<Cache>) -> Self {
		Self {
			target: target,
			cache,
			sanitizer: Sanitizer::new(target),
		}
	}

	/// Gets the sanitizer for this compiler
	pub fn sanitizer(&self) -> Sanitizer { self.sanitizer }

	/// Sanitizes text using this compiler's [`sanitizer`]
	pub fn sanitize<S: AsRef<str>>(&self, str: S) -> String { self.sanitizer.sanitize(str) }

	/// Sanitizes a format string for a [`Target`]
	///
	/// # Notes
	///
	/// This function may process invalid format string, which will be caught later
	/// by runtime_format.
	pub fn sanitize_format<S: AsRef<str>>(&self, str: S) -> String {
		self.sanitizer.sanitize_format(str)
	}

	/// Gets a reference name
	pub fn refname<S: AsRef<str>>(&self, str: S) -> String {
		self.sanitizer.sanitize(str).replace(' ', "_")
	}

	pub fn target(&self) -> Target { self.target }

	pub fn cache(&self) -> Arc<Cache> { self.cache.clone() }

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
						self.sanitizer.sanitize(page_title.to_string())
					)
					.as_str();
				}

				if let Some(css) = document.get_variable("html.css") {
					result += format!(
						"<link rel=\"stylesheet\" href=\"{}\">",
						self.sanitizer.sanitize(css.to_string())
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

	pub fn compile(
		&self,
		document: &dyn Document,
		colors: &ReportColors,
	) -> (CompiledDocument, PostProcess) {
		let borrow = document.content().borrow();

		// Header
		let header = self.header(document);

		// Body
		let output = CompilerOutput::run_with_processor(colors, |mut output| {
			{
				output.add_content(r#"<div class="content">"#);
				for elem in borrow.iter() {
					if let Err(reports) = elem.compile(self, document, &mut output) {
						Report::reports_to_stdout(colors, reports);
					};
				}
				output.add_content(r#"</div>"#);
			}
			output
		});

		// Footer
		let footer = self.footer(document);

		output.to_compiled(self, document, header, footer)
	}
}

#[derive(Debug)]
pub struct CompiledDocument {
	/// Input path relative to the input directory
	pub input: String,
	/// Modification time (i.e seconds since last epoch)
	///
	/// The purpose of this field is to know when to skip processing a document
	/// I.E the file's mtime is compared with the value in database
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

	pub fn init_cache(con: &MutexGuard<'_, Connection>) -> Result<usize, rusqlite::Error> {
		con.execute(Self::sql_table(), [])
	}

	pub fn from_cache(con: &MutexGuard<'_, Connection>, input: &str) -> Option<Self> {
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
		let sanitizer = Sanitizer::new(Target::HTML);

		assert_eq!(sanitizer.sanitize("<a>"), "&lt;a&gt;");
		assert_eq!(sanitizer.sanitize("&lt;"), "&amp;lt;");
		assert_eq!(sanitizer.sanitize("\""), "&quot;");

		assert_eq!(sanitizer.sanitize_format("{<>&\"}"), "{<>&\"}");
		assert_eq!(sanitizer.sanitize_format("{{<>}}"), "{{&lt;&gt;}}");
		assert_eq!(sanitizer.sanitize_format("{{<"), "{{&lt;");
	}
}
