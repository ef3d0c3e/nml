use std::collections::HashMap;
use std::rc::Rc;

use parser::parser::SharedState;
use parser::util::escape_source;
use reference_style::ExternalReferenceStyle;
use regex::Captures;
use regex::Regex;
use runtime_format::FormatArgs;
use runtime_format::FormatKey;
use runtime_format::FormatKeyError;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::CrossReference;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::document::references::validate_refname;
use crate::lsp::semantic::Semantics;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct InternalReference {
	pub(self) location: Token,
	pub(self) refname: String,
	pub(self) caption: Option<String>,
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
		_cursor: usize,
	) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
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
				)
			}
			_ => todo!(""),
		}
	}
}

#[derive(Debug)]
pub struct ExternalReference {
	pub(self) location: Token,
	pub(self) reference: CrossReference,
	pub(self) caption: Option<String>,
	pub(self) style: Rc<reference_style::ExternalReferenceStyle>,
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
			Target::HTML => {
				let mut result = "<a href=\"".to_string();

				// Link position
				let crossreference_pos = cursor + result.len();

				if let Some(caption) = &self.caption {
					result +=
						format!("\">{}</a>", Compiler::sanitize(Target::HTML, caption)).as_str();
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

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::reference")]
pub struct ReferenceRule {
	re: [Regex; 1],
	properties: PropertyParser,
}

impl Default for ReferenceRule {
	fn default() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"caption".to_string(),
			Property::new("Override the display of the reference".to_string(), None),
		);
		Self {
			re: [Regex::new(r"&\{(.*?)\}(?:\[((?:\\.|[^\\\\])*?)\])?").unwrap()],
			properties: PropertyParser { properties: props },
		}
	}
}

impl RegexRule for ReferenceRule {
	fn name(&self) -> &'static str { "Reference" }

	fn previous(&self) -> Option<&'static str> { Some("Text") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn enabled(&self, _mode: &ParseMode, _id: usize) -> bool { true }

	fn on_regex_match<'a>(
		&self,
		_: usize,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		let (refdoc, refname) = if let Some(refname_match) = matches.get(1) {
			if let Some(sep) = refname_match.as_str().find('#')
			// External reference
			{
				let refdoc = refname_match.as_str().split_at(sep).0;
				match validate_refname(document, refname_match.as_str().split_at(sep + 1).1, false)
				{
					Err(err) => {
						report_err!(
							&mut reports,
							token.source(),
							"Invalid Reference Refname".into(),
							span(refname_match.range(), err)
						);
						return reports;
					}
					Ok(refname) => (Some(refdoc.to_string()), refname.to_string()),
				}
			} else
			// Internal reference
			{
				match validate_refname(document, refname_match.as_str(), false) {
					Err(err) => {
						report_err!(
							&mut reports,
							token.source(),
							"Invalid Reference Refname".into(),
							span(refname_match.range(), err)
						);
						return reports;
					}
					Ok(refname) => (None, refname.to_string()),
				}
			}
		} else {
			panic!("Unknown error")
		};

		// Properties
		let prop_source = escape_source(
			token.source(),
			matches.get(2).map_or(0..0, |m| m.range()),
			"Reference Properties".into(),
			'\\',
			"]",
		);
		let properties = match self.properties.parse(
			"Reference",
			&mut reports,
			state,
			Token::new(0..prop_source.content().len(), prop_source),
		) {
			Some(props) => props,
			None => return reports,
		};

		let caption = match properties.get_opt(&mut reports, "caption", |_, value| {
			Result::<_, String>::Ok(value.value.clone())
		}) {
			Some(caption) => caption,
			None => return reports,
		};

		if let Some(refdoc) = refdoc {
			// Get style
			let style = state
				.shared
				.styles
				.borrow()
				.current(reference_style::STYLE_KEY)
				.downcast_rc::<reference_style::ExternalReferenceStyle>()
				.unwrap();

			// &{#refname}
			if refdoc.is_empty() {
				state.push(
					document,
					Box::new(ExternalReference {
						location: token.clone(),
						reference: CrossReference::Unspecific(refname),
						caption,
						style,
					}),
				);
			// &{docname#refname}
			} else {
				state.push(
					document,
					Box::new(ExternalReference {
						location: token.clone(),
						reference: CrossReference::Specific(refdoc.clone(), refname),
						caption,
						style,
					}),
				);
			}

			if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp)
			{
				let link = matches.get(1).unwrap().range();
				sems.add(link.start - 2..link.start - 1, tokens.reference_operator);
				sems.add(link.start - 1..link.start, tokens.reference_link_sep);

				if !refdoc.is_empty() {
					sems.add(link.start..refdoc.len() + link.start, tokens.reference_doc);
				}
				sems.add(
					refdoc.len() + link.start..refdoc.len() + link.start + 1,
					tokens.reference_doc_sep,
				);
				sems.add(
					refdoc.len() + link.start + 1..link.end,
					tokens.reference_link,
				);
				sems.add(link.end..link.end + 1, tokens.reference_link_sep);
			}
		} else {
			state.push(
				document,
				Box::new(InternalReference {
					location: token.clone(),
					refname,
					caption,
				}),
			);

			if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp)
			{
				let link = matches.get(1).unwrap().range();
				sems.add(link.start - 2..link.start - 1, tokens.reference_operator);
				sems.add(link.start - 1..link.start, tokens.reference_link_sep);
				sems.add(link.clone(), tokens.reference_link);
				sems.add(link.end..link.end + 1, tokens.reference_link_sep);
			}
		}

		if let (Some((sems, tokens)), Some(props)) = (
			Semantics::from_source(token.source(), &state.shared.lsp),
			matches.get(2).map(|m| m.range()),
		) {
			sems.add(props.start - 1..props.start, tokens.reference_props_sep);
			sems.add(props.end..props.end + 1, tokens.reference_props_sep);
		}

		reports
	}

	fn register_shared_state(&self, state: &SharedState) {
		let mut holder = state.styles.borrow_mut();
		holder.set_current(Rc::new(ExternalReferenceStyle::default()));
	}
}

mod reference_style {
	use serde::Deserialize;
	use serde::Serialize;

	use crate::impl_elementstyle;

	pub static STYLE_KEY: &str = "style.external_reference";

	#[derive(Debug, Serialize, Deserialize)]
	pub struct ExternalReferenceStyle {
		pub format_unspecific: String,
		pub format_specific: String,
	}

	impl Default for ExternalReferenceStyle {
		fn default() -> Self {
			Self {
				format_unspecific: "(#{refname})".into(),
				format_specific: "({refdoc}#{refname})".into(),
			}
		}
	}

	impl_elementstyle!(ExternalReferenceStyle, STYLE_KEY);
}

#[cfg(test)]
mod tests {
	use crate::compiler::process::process_from_memory;
	use crate::elements::paragraph::Paragraph;
	use crate::elements::section::Section;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;

	use super::*;

	#[test]
	pub fn parse_internal() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
#{ref} Referenceable section

&{ref}[caption=Section]
&{ref}[caption=Another]
&{ref2}[caption=Before]

#{ref2} Another section
"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(
			ParserState::new(&parser, None),
			source,
			None,
			ParseMode::default(),
		);

		validate_document!(doc.content().borrow(), 0,
			Section;
			Paragraph {
				InternalReference { refname == "ref", caption == Some("Section".to_string()) };
				InternalReference { refname == "ref", caption == Some("Another".to_string()) };
				InternalReference { refname == "ref2", caption == Some("Before".to_string()) };
			};
			Paragraph;
			Section;
		);
	}

	#[test]
	pub fn parse_external() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
&{DocA#ref}[caption=Section]
&{DocB#ref}
&{#ref}[caption='ref' from any document]
"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(
			ParserState::new(&parser, None),
			source,
			None,
			ParseMode::default(),
		);

		validate_document!(doc.content().borrow(), 0,
			Paragraph {
				ExternalReference { reference == CrossReference::Specific("DocA".into(), "ref".into()), caption == Some("Section".to_string()) };
				ExternalReference { reference == CrossReference::Specific("DocB".into(), "ref".into()), caption == None::<String> };
				ExternalReference { reference == CrossReference::Unspecific("ref".into()), caption == Some("'ref' from any document".to_string()) };
			};
		);
	}

	#[test]
	pub fn test_external() {
		let result = process_from_memory(
			Target::HTML,
			vec![
				r#"
@html.page_title = 0
@compiler.output = a.html

#{ref} Referenceable section
"#
				.into(),
				r#"
@html.page_title = 1
@compiler.output = b.html

&{#ref}
&{a#ref}
#{ref2} Another Referenceable section
"#
				.into(),
				r#"
@html.page_title = 2

@@style.external_reference = {
	"format_unspecific": "[UNSPECIFIC {refname}]",
	"format_specific": "[SPECIFIC {refdoc}:{refname}]"
}

&{#ref}[caption=from 0]
&{#ref}
&{#ref2}[caption=from 1]
&{b#ref2}
"#
				.into(),
			],
		)
		.unwrap();

		assert!(result[1].0.borrow().body.starts_with("<div class=\"content\"><p><a href=\"a.html#Referenceable_section\">(#ref)</a><a href=\"a.html#Referenceable_section\">(a#ref)</a></p>"));
		assert!(result[2].0.borrow().body.starts_with("<div class=\"content\"><p><a href=\"a.html#Referenceable_section\">from 0</a><a href=\"a.html#Referenceable_section\">[UNSPECIFIC ref]</a><a href=\"b.html#Another_Referenceable_section\">from 1</a><a href=\"b.html#Another_Referenceable_section\">[SPECIFIC b:ref2]</a></p>"));
	}
}
