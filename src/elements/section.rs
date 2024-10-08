use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::document::element::ReferenceableElement;
use crate::lua::kernel::CTX;
use crate::parser::parser::ParserState;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::style::StyleHolder;
use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use regex::Regex;
use section_style::SectionLinkPos;
use section_style::SectionStyle;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use super::reference::InternalReference;

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
	pub style: Rc<section_style::SectionStyle>,
}

impl Element for Section {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Block }
	fn element_name(&self) -> &'static str { "Section" }
	fn compile(&self, compiler: &Compiler, _document: &dyn Document, _cursor: usize) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
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
					return Ok(format!(
						r#"<h{0} id="{1}">{number}{2}</h{0}>"#,
						self.depth,
						Compiler::refname(compiler.target(), self.title.as_str()),
						Compiler::sanitize(compiler.target(), self.title.as_str())
					));
				}

				let refname = Compiler::refname(compiler.target(), self.title.as_str());
				let link = format!(
					"{}<a class=\"section-link\" href=\"#{refname}\">{}</a>{}",
					Compiler::sanitize(compiler.target(), self.style.link[0].as_str()),
					Compiler::sanitize(compiler.target(), self.style.link[1].as_str()),
					Compiler::sanitize(compiler.target(), self.style.link[2].as_str())
				);

				if self.style.link_pos == SectionLinkPos::After {
					Ok(format!(
						r#"<h{0} id="{1}">{number}{2}{link}</h{0}>"#,
						self.depth,
						Compiler::refname(compiler.target(), self.title.as_str()),
						Compiler::sanitize(compiler.target(), self.title.as_str())
					))
				} else
				// Before
				{
					Ok(format!(
						r#"<h{0} id="{1}">{link}{number}{2}</h{0}>"#,
						self.depth,
						Compiler::refname(compiler.target(), self.title.as_str()),
						Compiler::sanitize(compiler.target(), self.title.as_str())
					))
				}
			}
			Target::LATEX => Err("Unimplemented compiler".to_string()),
		}
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
			Target::HTML => {
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

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::section")]
pub struct SectionRule {
	re: [Regex; 1],
}

impl SectionRule {
	pub fn new() -> Self {
		Self {
			re: [Regex::new(r"(?:^|\n)(#{1,})(?:\{(.*)\})?((\*|\+){1,})?(.*)").unwrap()],
		}
	}
}

pub mod section_kind {
	pub const NONE: u8 = 0x00;
	pub const NO_TOC: u8 = 0x01;
	pub const NO_NUMBER: u8 = 0x02;
}

impl RegexRule for SectionRule {
	fn name(&self) -> &'static str { "Section" }
	fn previous(&self) -> Option<&'static str> { Some("Custom Style") }

	fn regexes(&self) -> &[Regex] { &self.re }

	fn on_regex_match(
		&self,
		_: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: regex::Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut result = vec![];
		let section_depth = match matches.get(1) {
			Some(depth) => {
				if depth.len() > 6 {
					result.push(
					Report::build(ReportKind::Error, token.source(), depth.start())
						.with_message("Invalid section depth")
						.with_label(
							Label::new((token.source(), depth.range()))
							.with_message(format!("Section is of depth {}, which is greather than {} (maximum depth allowed)",
                            depth.len().fg(state.parser.colors().info),
                            6.fg(state.parser.colors().info)))
							.with_color(state.parser.colors().error))
						.finish());
					return result;
				}

				depth.len()
			}
			_ => panic!("Empty section depth"),
		};

		// [Optional] Reference name
		let section_refname =
			matches.get(2).map_or_else(
				|| None,
				|refname| {
					// Check for duplicate reference
					if let Some(elem_reference) = document.get_reference(refname.as_str()) {
						let elem = document.get_from_reference(&elem_reference).unwrap();

						result.push(
						Report::build(ReportKind::Warning, token.source(), refname.start())
						.with_message("Duplicate reference name")
						.with_label(
							Label::new((token.source(), refname.range()))
							.with_message(format!("Reference with name `{}` is already defined in `{}`",
									refname.as_str().fg(state.parser.colors().highlight),
									elem.location().source().name().as_str().fg(state.parser.colors().highlight)))
							.with_message(format!("`{}` conflicts with previously defined reference to {}",
									refname.as_str().fg(state.parser.colors().highlight),
									elem.element_name().fg(state.parser.colors().highlight)))
							.with_color(state.parser.colors().warning))
						.with_label(
							Label::new((elem.location().source(), elem.location().start()..elem.location().end() ))
							.with_message(format!("`{}` previously defined here",
								refname.as_str().fg(state.parser.colors().highlight)))
							.with_color(state.parser.colors().warning))
						.with_note("Previous reference was overwritten".to_string())
						.finish());
					}
					Some(refname.as_str().to_string())
				},
			);

		// Section kind
		let section_kind = match matches.get(3) {
			Some(kind) => match kind.as_str() {
				"*+" | "+*" => section_kind::NO_NUMBER | section_kind::NO_TOC,
				"*" => section_kind::NO_NUMBER,
				"+" => section_kind::NO_TOC,
				"" => section_kind::NONE,
				_ => {
					result.push(
							Report::build(ReportKind::Error, token.source(), kind.start())
							.with_message("Invalid section numbering kind")
							.with_label(
								Label::new((token.source(), kind.range()))
								.with_message(format!("Section numbering kind must be a combination of `{}` for unnumbered, and `{}` for non-listing; got `{}`",
										"*".fg(state.parser.colors().info),
										"+".fg(state.parser.colors().info),
										kind.as_str().fg(state.parser.colors().highlight)))
								.with_color(state.parser.colors().error))
								.with_help("Leave empty for a numbered listed section".to_string())
							.finish());
					return result;
				}
			},
			_ => section_kind::NONE,
		};

		// Spacing + Section name
		let section_name = match matches.get(5) {
			Some(name) => {
				let split = name
					.as_str()
					.chars()
					.position(|c| !c.is_whitespace())
					.unwrap_or(0);

				let section_name = &name.as_str()[split..];
				if section_name.is_empty()
				// No name
				{
					result.push(
						Report::build(ReportKind::Error, token.source(), name.start())
							.with_message("Missing section name")
							.with_label(
								Label::new((token.source(), name.range()))
									.with_message("Sections require a name before line end")
									.with_color(state.parser.colors().error),
							)
							.finish(),
					);
					return result;
				}

				// No spacing
				if split == 0 {
					result.push(
						Report::build(ReportKind::Warning, token.source(), name.start())
						.with_message("Missing section spacing")
						.with_label(
							Label::new((token.source(), name.range()))
							.with_message("Sections require at least one whitespace before the section's name")
							.with_color(state.parser.colors().warning))
                        .with_help(format!("Add a space before `{}`", section_name.fg(state.parser.colors().highlight)))
						.finish());
					return result;
				}

				section_name.to_string()
			}
			_ => panic!("Empty section name"),
		};

		// Get style
		let style = state
			.shared
			.styles
			.borrow()
			.current(section_style::STYLE_KEY)
			.downcast_rc::<SectionStyle>()
			.unwrap();

		state.push(
			document,
			Box::new(Section {
				location: token.clone(),
				title: section_name,
				depth: section_depth,
				kind: section_kind,
				reference: section_refname,
				style,
			}),
		);

		result
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];

		bindings.push((
			"push".to_string(),
			lua.create_function(
				|_, (title, depth, kind, reference): (String, usize, Option<String>, Option<String>)| {
					let kind = match kind.as_deref().unwrap_or("") {
						"*+" | "+*" => section_kind::NO_NUMBER | section_kind::NO_TOC,
						"*" => section_kind::NO_NUMBER,
						"+" => section_kind::NO_TOC,
						"" => section_kind::NONE,
						_ => {
							return Err(BadArgument {
								to: Some("push".to_string()),
								pos: 3,
								name: Some("kind".to_string()),
								cause: Arc::new(mlua::Error::external("Unknown section kind specified".to_string())),
							})
						}
					};

					CTX.with_borrow(|ctx| {
						ctx.as_ref().map(|ctx| {
							// Get style
							let style = ctx
								.state
								.shared
								.styles
								.borrow()
								.current(section_style::STYLE_KEY)
								.downcast_rc::<SectionStyle>()
								.unwrap();

							ctx.state.push(
								ctx.document,
								Box::new(Section {
									location: ctx.location.clone(),
									title,
									depth,
									kind,
									reference,
									style,
								}),
							);
						})
					});

					Ok(())
				},
			)
			.unwrap(),
		));

		bindings
	}

	fn register_styles(&self, holder: &mut StyleHolder) {
		holder.set_current(Rc::new(SectionStyle::default()));
	}
}

mod section_style {
	use serde::Deserialize;
	use serde::Serialize;

	use crate::impl_elementstyle;

	pub static STYLE_KEY: &str = "style.section";

	#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
	pub enum SectionLinkPos {
		Before,
		After,
		None,
	}

	#[derive(Debug, Serialize, Deserialize)]
	pub struct SectionStyle {
		pub link_pos: SectionLinkPos,
		pub link: [String; 3],
	}

	impl Default for SectionStyle {
		fn default() -> Self {
			Self {
				link_pos: SectionLinkPos::Before,
				link: ["".into(), "🔗".into(), " ".into()],
			}
		}
	}

	impl_elementstyle!(SectionStyle, STYLE_KEY);
}

#[cfg(test)]
mod tests {
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;

	use super::*;

	#[test]
	fn parser() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
# 1
##+ 2
###* 3
####+* 4
#####*+ 5
######{refname} 6
		"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

		validate_document!(doc.content().borrow(), 0,
			Section { depth == 1, title == "1" };
			Section { depth == 2, title == "2", kind == section_kind::NO_TOC };
			Section { depth == 3, title == "3", kind == section_kind::NO_NUMBER };
			Section { depth == 4, title == "4", kind == section_kind::NO_NUMBER | section_kind::NO_TOC };
			Section { depth == 5, title == "5", kind == section_kind::NO_NUMBER | section_kind::NO_TOC };
			Section { depth == 6, title == "6", reference == Some("refname".to_string()) };
		);
	}

	#[test]
	fn lua() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
%<
nml.section.push("1", 1, "", nil)
nml.section.push("2", 2, "+", nil)
nml.section.push("3", 3, "*", nil)
nml.section.push("4", 4, "+*", nil)
nml.section.push("5", 5, "*+", nil)
nml.section.push("6", 6, "", "refname")
>%
		"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

		validate_document!(doc.content().borrow(), 0,
			Section { depth == 1, title == "1" };
			Section { depth == 2, title == "2", kind == section_kind::NO_TOC };
			Section { depth == 3, title == "3", kind == section_kind::NO_NUMBER };
			Section { depth == 4, title == "4", kind == section_kind::NO_NUMBER | section_kind::NO_TOC };
			Section { depth == 5, title == "5", kind == section_kind::NO_NUMBER | section_kind::NO_TOC };
			Section { depth == 6, title == "6", reference == Some("refname".to_string()) };
		);
	}

	#[test]
	fn style() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
@@style.section = {
	"link_pos": "None",
	"link": ["a", "b", "c"]
}
		"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let state = ParserState::new(&parser, None);
		let (_, state) = parser.parse(state, source, None);

		let style = state.shared
			.styles
			.borrow()
			.current(section_style::STYLE_KEY)
			.downcast_rc::<SectionStyle>()
			.unwrap();

		assert_eq!(style.link_pos, SectionLinkPos::None);
		assert_eq!(style.link, ["a".to_string(), "b".to_string(), "c".to_string()]);
	}
}
