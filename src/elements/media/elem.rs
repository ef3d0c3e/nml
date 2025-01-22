use std::str::FromStr;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::CompilerOutput;
use crate::compiler::compiler::Target;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::document::element::ReferenceableElement;
use crate::elements::paragraph::elem::Paragraph;
use crate::elements::reference::elem::InternalReference;
use crate::parser::reports::Report;
use crate::parser::source::Token;

#[derive(Debug, PartialEq, Eq)]
pub enum MediaType {
	IMAGE,
	VIDEO,
	AUDIO,
}

impl FromStr for MediaType {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"image" => Ok(MediaType::IMAGE),
			"video" => Ok(MediaType::VIDEO),
			"audio" => Ok(MediaType::AUDIO),
			_ => Err(format!("Unknown media type: {s}")),
		}
	}
}

#[derive(Debug)]
pub struct Media {
	pub(crate) location: Token,
	pub(crate) media: Vec<Box<dyn Element>>,
}

impl Element for Media {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Block }

	fn element_name(&self) -> &'static str { "Media" }

	fn as_container(&self) -> Option<&dyn ContainerElement> { Some(self) }

	fn compile<'e>(
		&'e self,
		compiler: &'e Compiler,
		document: &'e dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				output.add_content("<div class=\"media\">");
				for medium in &self.media {
					medium.compile(compiler, document, output)?;
				}
				output.add_content("</div>");
			}
			_ => todo!(""),
		}
		Ok(())
	}
}

impl ContainerElement for Media {
	fn contained(&self) -> &Vec<Box<dyn Element>> { &self.media }

	fn push(&mut self, elem: Box<dyn Element>) -> Result<(), String> {
		let medium = match elem.downcast_ref::<Medium>() {
			Some(medium) => medium,
			None => return Err("Attempted to insert invalid element into Media".to_string()),
		};
		if self.location.source() != medium.location.source() {
			return Err(format!(
				"Attempted to insert medium from {} into medium from {}",
				self.location.source(),
				medium.location.source()
			));
		}

		self.location.range = self.location.start()..medium.location.end();
		self.media.push(elem);
		Ok(())
	}
}

#[derive(Debug)]
pub struct Medium {
	pub(crate) location: Token,
	pub(crate) reference: String,
	pub(crate) uri: String,
	pub(crate) media_type: MediaType,
	pub(crate) width: Option<String>,
	pub(crate) caption: Option<String>,
	pub(crate) description: Option<Paragraph>,
}

impl Element for Medium {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Block }

	fn element_name(&self) -> &'static str { "Medium" }

	fn as_referenceable(&self) -> Option<&dyn ReferenceableElement> { Some(self) }

	fn compile<'e>(
		&'e self,
		compiler: &'e Compiler,
		document: &'e dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			Target::HTML => {
				// Reference
				let elemref = document.get_reference(self.reference.as_str()).unwrap();
				let refcount = compiler.reference_id(document, elemref);

				let width = self
					.width
					.as_ref()
					.map_or(String::new(), |w| format!(r#" style="width:{w};""#));
				output.add_content(format!(
					r#"<div id="{}" class="medium"{width}>"#,
					self.refid(compiler, refcount)
				));
				output.add_content(match self.media_type {
					MediaType::IMAGE => format!(r#"<a href="{0}"><img src="{0}"></a>"#, self.uri),
					MediaType::VIDEO => format!(
						r#"<video controls{width}><source src="{0}"></video>"#,
						self.uri
					),
					MediaType::AUDIO => {
						format!(r#"<audio controls src="{0}"{width}></audio>"#, self.uri)
					}
				});

				let caption = self
					.caption
					.as_ref()
					.map(|cap| format!(" {}", compiler.sanitize(cap.as_str())))
					.unwrap_or_default();

				output.add_content(format!(
					r#"<p class="medium-refname">({refcount}) {caption}</p>"#
				));
				if let Some(paragraph) = self.description.as_ref() {
					paragraph.compile(compiler, document, output)?;
				}
				output.add_content("</div>");
			}
			_ => todo!(""),
		}
		Ok(())
	}
}

impl ReferenceableElement for Medium {
	fn reference_name(&self) -> Option<&String> { Some(&self.reference) }

	fn refcount_key(&self) -> &'static str { "medium" }

	fn compile_reference(
		&self,
		compiler: &Compiler,
		_document: &dyn Document,
		reference: &InternalReference,
		refid: usize,
	) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				let caption = reference
					.caption()
					.map_or(format!("({refid})"), |cap| cap.clone());

				// TODO Handle other kind of media
				match self.media_type {
					MediaType::IMAGE => Ok(format!(
						"<a class=\"medium-ref\" href=\"#{}\">{caption}<img src=\"{}\"></a>",
						self.refid(compiler, refid),
						self.uri
					)),
					MediaType::VIDEO => Ok(format!(
						"<a class=\"medium-ref\" href=\"#{}\">{caption}<video><source src=\"{}\"></video></a>",
						self.refid(compiler, refid),
						self.uri
					)),
					_ => todo!(""),
				}
			}
			_ => todo!(""),
		}
	}

	fn refid(&self, _compiler: &Compiler, refid: usize) -> String { format!("medium-{refid}") }
}
