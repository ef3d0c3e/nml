use std::rc::Rc;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::CompilerOutput;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::document::element::ReferenceableElement;
use crate::elements::reference::elem::InternalReference;
use crate::parser::reports::Report;
use crate::parser::source::Token;

use super::rule::section_kind;
use super::style::SectionLinkPos;
use super::style::SectionStyle;

#[derive(Debug)]
pub struct Section {
	pub location: Token,
	/// Title of the section
	pub title: String,
	/// Depth i.e number of '#'
	pub depth: usize,
	/// [`section_kind`]
	pub kind: u8,
	/// Section reference name
	pub reference: Option<String>,
	/// Style of the section
	pub style: Rc<SectionStyle>,
}

impl Element for Section {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Block }
	fn element_name(&self) -> &'static str { "Section" }
	fn compile<'e>(
		&self,
		compiler: &Compiler,
		_document: &dyn Document,
		mut output: CompilerOutput,
	) -> Result<CompilerOutput, Vec<Report>> {
		match compiler.target() {
			HTML => {
				// Section numbering
				let number = if (self.kind & section_kind::NO_NUMBER) != section_kind::NO_NUMBER {
					let numbering = compiler.section_counter(self.depth);

					let mut result = String::new();
					for num in numbering.iter() {
						result = result + num.to_string().as_str() + ".";
					}
					result += " ";

					result
				} else {
					String::new()
				};

				if self.style.link_pos == SectionLinkPos::None {
					output.add_content(format!(
						r#"<h{0} id="{1}">{number}{2}</h{0}>"#,
						self.depth,
						Compiler::refname(compiler.target(), self.title.as_str()),
						Compiler::sanitize(compiler.target(), self.title.as_str())
					));
					return Ok(output);
				}

				let refname = Compiler::refname(compiler.target(), self.title.as_str());
				let link = format!(
					"{}<a class=\"section-link\" href=\"#{refname}\">{}</a>{}",
					Compiler::sanitize(compiler.target(), self.style.link[0].as_str()),
					Compiler::sanitize(compiler.target(), self.style.link[1].as_str()),
					Compiler::sanitize(compiler.target(), self.style.link[2].as_str())
				);

				if self.style.link_pos == SectionLinkPos::After {
					output.add_content(format!(
						r#"<h{0} id="{1}">{number}{2}{link}</h{0}>"#,
						self.depth,
						Compiler::refname(compiler.target(), self.title.as_str()),
						Compiler::sanitize(compiler.target(), self.title.as_str())
					));
				} else
				// Before
				{
					output.add_content(format!(
						r#"<h{0} id="{1}">{link}{number}{2}</h{0}>"#,
						self.depth,
						Compiler::refname(compiler.target(), self.title.as_str()),
						Compiler::sanitize(compiler.target(), self.title.as_str())
					))
				}
			}
			_ => todo!(""),
		}
		Ok(output)
	}

	fn as_referenceable(&self) -> Option<&dyn ReferenceableElement> { Some(self) }
}

impl ReferenceableElement for Section {
	fn reference_name(&self) -> Option<&String> { self.reference.as_ref() }

	fn refcount_key(&self) -> &'static str { "section" }

	fn compile_reference(
		&self,
		compiler: &Compiler,
		_document: &dyn Document,
		reference: &InternalReference,
		_refid: usize,
	) -> Result<String, String> {
		match compiler.target() {
			HTML => {
				let caption = reference.caption().map_or(
					format!(
						"({})",
						Compiler::sanitize(compiler.target(), self.title.as_str())
					),
					|cap| cap.clone(),
				);

				Ok(format!(
					"<a class=\"section-reference\" href=\"#{}\">{caption}</a>",
					Compiler::refname(compiler.target(), self.title.as_str())
				))
			}
			_ => todo!(""),
		}
	}

	fn refid(&self, compiler: &Compiler, _refid: usize) -> String {
		Compiler::refname(compiler.target(), self.title.as_str())
	}
}
