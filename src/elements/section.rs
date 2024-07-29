use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::document::element::ReferenceableElement;
use crate::lua::kernel::CTX;
use crate::parser::parser::Parser;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use regex::Regex;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug)]
pub struct Section {
	pub(self) location: Token,
	pub(self) title: String,             // Section title
	pub(self) depth: usize,              // Section depth
	pub(self) kind: u8,                  // Section kind, e.g numbered, unnumbred, ...
	pub(self) reference: Option<String>, // Section reference name
}

impl Element for Section {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Block }
	fn element_name(&self) -> &'static str { "Section" }
	fn to_string(&self) -> String { format!("{self:#?}") }
	fn as_referenceable(&self) -> Option<&dyn ReferenceableElement> { Some(self) }
	fn compile(&self, compiler: &Compiler, _document: &dyn Document) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => Ok(format!(
				"<h{0}>{1}</h{0}>",
				self.depth,
				Compiler::sanitize(compiler.target(), self.title.as_str())
			)),
			Target::LATEX => Err("Unimplemented compiler".to_string()),
		}
	}
}

impl ReferenceableElement for Section {
	fn reference_name(&self) -> Option<&String> { self.reference.as_ref() }

	fn refcount_key(&self) -> &'static str { "section" }

	fn compile_reference(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		reference: &super::reference::Reference,
		refid: usize,
	) -> Result<String, String> {
		todo!()
	}
}

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

	fn regexes(&self) -> &[Regex] { &self.re }

	fn on_regex_match(
		&self,
		_: usize,
		parser: &dyn Parser,
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
                            depth.len().fg(parser.colors().info),
                            6.fg(parser.colors().info)))
							.with_color(parser.colors().error))
						.finish());
					return result;
				}

				depth.len()
			}
			_ => panic!("Empty section depth"),
		};

		// [Optional] Reference name
		let section_refname = matches.get(2).map_or_else(
			|| None,
			|refname| {
				/* TODO: Wait for reference rework
				// Check for duplicate reference
				if let Some((ref_doc, reference)) = document.get_reference(refname.as_str())
				{
					result.push(
						Report::build(ReportKind::Warning, token.source(), refname.start())
						.with_message("Duplicate reference name")
						.with_label(
							Label::new((token.source(), refname.range()))
							.with_message(format!("Reference with name `{}` is already defined in `{}`",
									refname.as_str().fg(parser.colors().highlight),
									ref_doc.source().name().as_str().fg(parser.colors().highlight)))
							.with_message(format!("`{}` conflicts with previously defined reference to {}",
									refname.as_str().fg(parser.colors().highlight),
									reference.element_name().fg(parser.colors().highlight)))
							.with_color(parser.colors().warning))
						.with_label(
							Label::new((ref_doc.source(), reference.location().start()+1..reference.location().end() ))
							.with_message(format!("`{}` previously defined here",
								refname.as_str().fg(parser.colors().highlight)))
							.with_color(parser.colors().warning))
						.with_note(format!("Previous reference was overwritten"))
						.finish());
				}
				*/
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
										"*".fg(parser.colors().info),
										"+".fg(parser.colors().info),
										kind.as_str().fg(parser.colors().highlight)))
								.with_color(parser.colors().error))
								.with_help(format!("Leave empty for a numbered listed section"))
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
									.with_color(parser.colors().error),
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
							.with_color(parser.colors().warning))
                        .with_help(format!("Add a space before `{}`", section_name.fg(parser.colors().highlight)))
						.finish());
					return result;
				}

				section_name.to_string()
			}
			_ => panic!("Empty section name"),
		};

		parser.push(
			document,
			Box::new(Section {
				location: token.clone(),
				title: section_name,
				depth: section_depth,
				kind: section_kind,
				reference: section_refname,
			}),
		);

		return result;
	}

	fn lua_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];

		bindings.push((
			"push".to_string(),
			lua.create_function(
				|_, (title, depth, kind, reference): (String, usize, String, Option<String>)| {
					let kind = match kind.as_str() {
						"*+" | "+*" => section_kind::NO_NUMBER | section_kind::NO_TOC,
						"*" => section_kind::NO_NUMBER,
						"+" => section_kind::NO_TOC,
						"" => section_kind::NONE,
						_ => {
							return Err(BadArgument {
								to: Some("push".to_string()),
								pos: 3,
								name: Some("kind".to_string()),
								cause: Arc::new(mlua::Error::external(format!(
									"Unknown section kind specified"
								))),
							})
						}
					};

					CTX.with_borrow(|ctx| {
						ctx.as_ref().map(|ctx| {
							ctx.parser.push(
								ctx.document,
								Box::new(Section {
									location: ctx.location.clone(),
									title,
									depth,
									kind,
									reference,
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
}
