use std::sync::Arc;

use parking_lot::RwLock;

use crate::cache::cache::Cache;
use crate::parser::reports::Report;
use crate::unit::element::nested_kind;
use crate::unit::element::ElemKind;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;
use crate::util::settings::ProjectOutput;

use super::output::CompilerOutput;
use super::sanitize::Sanitizer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
	HTML,
	#[allow(unused)]
	LATEX,
}

impl From<&ProjectOutput> for Target {
	fn from(value: &ProjectOutput) -> Self {
		match value {
			ProjectOutput::Html(_) => Target::HTML,
		}
	}
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
	pub fn sanitizer(&self) -> Sanitizer {
		self.sanitizer
	}

	/// Sanitizes text using this compiler's [`sanitizer`]
	pub fn sanitize<S: AsRef<str>>(&self, str: S) -> String {
		self.sanitizer.sanitize(str)
	}

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
	pub fn target(&self) -> Target {
		self.target
	}

	pub fn compile_scope(
		&self,
		mut output: CompilerOutput,
		scope: Arc<RwLock<Scope>>,
	) -> CompilerOutput {
		let mut reports = vec![];
		for (scope, elem) in scope.content_iter(false) {
			if nested_kind(elem.clone()) == ElemKind::Inline && !output.in_paragraph(&scope) {
				match self.target {
					Target::HTML => output.add_content("" /*"<p>"*/),
					Target::LATEX => todo!(),
				}
				output.set_paragraph(&scope, true);
			} else if output.in_paragraph(&scope) && nested_kind(elem.clone()) != ElemKind::Inline {
				match self.target {
					Target::HTML => output.add_content("" /*"</p>"*/),
					Target::LATEX => todo!(),
				}
				output.set_paragraph(&scope, false);
			}

			if let Err(mut reps) = elem.compile(scope, self, &mut output) {
				reports.extend(reps.drain(..));
			}
		}
		println!("Output={}", output.content());
		output
	}

	fn header(&self, unit: &TranslationUnit) -> String {
		let settings = unit.get_settings();

		match self.target {
			Target::HTML => {
				let ProjectOutput::Html(html) = &settings.output else {
					panic!("Invalid project settings")
				};
				let css = if let Some(css) = &html.css {
					format!(
						"<link rel=\"stylesheet\" href=\"{}\">",
						self.sanitize(&css.display().to_string())
					)
				} else {
					"".into()
				};
				let icon = if let Some(icon) = &html.icon {
					format!(
						"<link rel=\"icon\" href=\"{}\">",
						self.sanitize(&icon.display().to_string())
					)
				} else {
					"".into()
				};
				format!(
					"<!DOCTYPE html><html lang=\"{}\"><head><meta charset=\"utf-8\">{icon}{css}</head><body>",
					self.sanitize(html.language.as_str())
				)
			}
			_ => todo!(),
		}
	}

	fn footer(&self, unit: &TranslationUnit) -> String {
		let settings = unit.get_settings();

		match self.target {
			Target::HTML => "</body></html>".into(),
			_ => todo!(),
		}
	}

	/// Compiles a document to it's output
	pub fn compile(&self, unit: &TranslationUnit) -> Result<String, Vec<Report>> {
		let body = CompilerOutput::run_with_processor(self.target, &unit.colors(), |output| {
			self.compile_scope(output, unit.get_entry_scope().to_owned())
		})?;

		let output = format!("{}<main><article>{}</article></main>{}", self.header(unit), body.content(), self.footer(unit));
		Ok(output)
	}

	pub fn get_cache(&self) -> Arc<Cache> {
		self.cache.clone()
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
