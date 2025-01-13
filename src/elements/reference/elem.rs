use std::rc::Rc;

use runtime_format::FormatArgs;
use runtime_format::FormatKey;
use runtime_format::FormatKeyError;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::CompilerOutput;
use crate::compiler::compiler::Target;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::document::references::CrossReference;
use crate::parser::reports::Report;
use crate::parser::source::Token;

use super::style::ExternalReferenceStyle;

#[derive(Debug)]
pub struct InternalReference {
	pub(crate) location: Token,
	pub(crate) refname: String,
	pub(crate) caption: Option<String>,
}

impl InternalReference {
	pub fn caption(&self) -> Option<&String> { self.caption.as_ref() }
}

impl Element for InternalReference {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Inline }

	fn element_name(&self) -> &'static str { "Reference" }

	fn compile(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				let elemref = document
					.get_reference(self.refname.as_str())
					.ok_or(format!(
						"Unable to find reference `{}` in current document",
						self.refname
					))?;
				let elem = document.get_from_reference(&elemref).unwrap();

				elem.compile_reference(
					compiler,
					document,
					self,
					compiler.reference_id(document, elemref),
				)?;
			}
			_ => todo!(""),
		}
		Ok(())
	}
}

#[derive(Debug)]
pub struct ExternalReference {
	pub(crate) location: Token,
	pub(crate) reference: CrossReference,
	pub(crate) caption: Option<String>,
	pub(crate) style: Rc<ExternalReferenceStyle>,
}

impl ExternalReference {
	pub fn style(&self) -> &Rc<ExternalReferenceStyle> { &self.style }
}

struct FmtPair<'a>(Target, &'a ExternalReference);

impl FormatKey for FmtPair<'_> {
	fn fmt(&self, key: &str, f: &mut std::fmt::Formatter<'_>) -> Result<(), FormatKeyError> {
		match &self.1.reference {
			CrossReference::Unspecific(refname) => match key {
				"refname" => write!(f, "{}", Compiler::sanitize(self.0, refname))
					.map_err(FormatKeyError::Fmt),
				_ => Err(FormatKeyError::UnknownKey),
			},
			CrossReference::Specific(refdoc, refname) => match key {
				"refdoc" => {
					write!(f, "{}", Compiler::sanitize(self.0, refdoc)).map_err(FormatKeyError::Fmt)
				}
				"refname" => write!(f, "{}", Compiler::sanitize(self.0, refname))
					.map_err(FormatKeyError::Fmt),
				_ => Err(FormatKeyError::UnknownKey),
			},
		}
	}
}

impl Element for ExternalReference {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Inline }

	fn element_name(&self) -> &'static str { "Reference" }

	fn compile(
		&self,
		compiler: &Compiler,
		_document: &dyn Document,
		cursor: usize,
	) -> Result<String, String> {
		match compiler.target() {
			HTML => {
				let mut result = "<a href=\"".to_string();

				// Link position
				let crossreference_pos = cursor + result.len();

				if let Some(caption) = &self.caption {
					result += format!("\">{}</a>", Compiler::sanitize(HTML, caption)).as_str();
				} else {
					// Use style
					let fmt_pair = FmtPair(compiler.target(), self);
					let format_string = match &self.reference {
						CrossReference::Unspecific(_) => Compiler::sanitize_format(
							fmt_pair.0,
							self.style.format_unspecific.as_str(),
						),
						CrossReference::Specific(_, _) => Compiler::sanitize_format(
							fmt_pair.0,
							self.style.format_specific.as_str(),
						),
					};
					let args = FormatArgs::new(format_string.as_str(), &fmt_pair);
					args.status().map_err(|err| {
						format!("Failed to format ExternalReference style `{format_string}`: {err}")
					})?;

					result += format!("\">{}</a>", args).as_str();
				}
				// Add crossreference
				compiler.insert_crossreference(crossreference_pos, self.reference.clone());
				Ok(result)
			}
			_ => todo!(""),
		}
	}
}
