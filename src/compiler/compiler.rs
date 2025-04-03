use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use graphviz_rust::print;

use crate::cache::cache::Cache;
use crate::parser::reports::Report;
use crate::unit::element::{nested_kind, ElemKind};
use crate::unit::scope::{Scope, ScopeAccessor};
use crate::unit::translation::TranslationUnit;

use super::output::CompilerOutput;
use super::sanitize::Sanitizer;

#[derive(Clone, Copy)]
pub enum Target {
	HTML,
	#[allow(unused)]
	LATEX,
}

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

	/// Gets the output target of this compiler
	pub fn target(&self) -> Target { self.target }

	/* FIXME
	/// Produces the header for a given document
	fn header(&self, document: &dyn Document) -> String {
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

	/// Produces the footer for a given document
	fn footer(&self, _document: &dyn Document) -> String {
		let mut result = String::new();
		match self.target() {
			Target::HTML => {
				result += "</div></body></html>";
			}
			Target::LATEX => todo!(""),
		}
		result
	}
	*/

	pub fn compile_scope(
		&self,
		mut output: CompilerOutput,
		scope: Rc<RefCell<Scope>>
		) -> CompilerOutput
	{
		let mut reports = vec![];
		for (scope, elem) in scope.content_iter(false)
		{
			println!("Compiling={elem:#?}");
			if nested_kind(elem.clone()) == ElemKind::Inline && !output.in_paragraph(&scope)
			{
				match self.target
				{
					Target::HTML => output.add_content("<p>"),
					Target::LATEX => todo!(),
				}
				output.set_paragraph(&scope, true);
			}
			else if output.in_paragraph(&scope) && nested_kind(elem.clone()) != ElemKind::Inline
			{
				match self.target
				{
					Target::HTML => output.add_content("</p>"),
					Target::LATEX => todo!(),
				}
				output.set_paragraph(&scope, false);
			}
				

			if let Err(mut reps) = elem.compile(scope, self, &mut output)
			{
				reports.extend(reps.drain(..));
			}
		}
		println!("Output={}", output.content());
		output
	}

	/// Compiles a document to it's output
	pub fn compile(
		&self,
		unit: &TranslationUnit,
	) -> ( /* TODO */ ) {
		CompilerOutput::run_with_processor(self.target, &unit.colors(), |output| {
			self.compile_scope(output, unit.get_entry_scope().to_owned())
		});
		/*
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
		*/
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
