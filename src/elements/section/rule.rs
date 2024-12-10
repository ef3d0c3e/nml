use mlua::Error::BadArgument;
use parser::rule::RegexRule;
use std::sync::Arc;

use crate::document::document::Document;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::reports::macros::*;
use crate::parser::reports::Report;
use crate::parser::reports::*;
use crate::parser::source::Token;
use ariadne::Fmt;
use lsp::semantic::Semantics;
use lua::kernel::CTX;
use mlua::Function;
use mlua::Lua;
use regex::Regex;

use super::elem::Section;
use super::style::SectionStyle;

#[auto_registry::auto_registry(registry = "rules")]
pub struct SectionRule {
	re: [Regex; 1],
}

impl Default for SectionRule {
	fn default() -> Self {
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

	fn enabled(&self, mode: &ParseMode, _id: usize) -> bool { !mode.paragraph_only }

	fn on_regex_match(
		&self,
		_: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: regex::Captures,
	) -> Vec<Report> {
		let mut reports = vec![];
		let section_depth = match matches.get(1) {
			Some(depth) => {
				if depth.len() > 6 {
					report_err!(
						&mut reports,
						token.source(),
						"Invalid Section Depth".into(),
						span(
							depth.range(),
							format!("Section is of depth {}, which is greather than {} (maximum depth allowed)",
							depth.len().fg(state.parser.colors().info),
							6.fg(state.parser.colors().info))
						)
					);
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

						report_warn!(
							&mut reports,
							token.source(),
							"Duplicate Reference Name".into(),
							span(
								refname.range(),
								format!("Reference with name `{}` is already defined in `{}`. `{}` conflicts with previously defined reference to {}",
									refname.as_str().fg(state.parser.colors().highlight),
									elem.location().source().name().as_str().fg(state.parser.colors().highlight),
									refname.as_str().fg(state.parser.colors().highlight),
									elem.element_name().fg(state.parser.colors().highlight))
							),
							span(
								elem.location().source(),
								elem.location().start()..elem.location().end(),
								format!("`{}` previously defined here",
									refname.as_str().fg(state.parser.colors().highlight))
							),
							note("Previous reference was overwritten".into())
						);
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
					report_err!(
						&mut reports,
						token.source(),
						"Invalid Section Numbering Kind".into(),
						span(
							kind.range(),
							format!("Section numbering kind must be a combination of `{}` for unnumbered, and `{}` for non-listing; got `{}`",
								"*".fg(state.parser.colors().info),
								"+".fg(state.parser.colors().info),
								kind.as_str().fg(state.parser.colors().highlight))
						),
						help("Leave empty for a numbered listed section".into())
					);
					return reports;
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
					report_err!(
						&mut reports,
						token.source(),
						"Missing Section Name".into(),
						span(
							name.range(),
							"Section name must be specified before line end".into()
						),
					);
					return reports;
				}

				// No spacing
				if split == 0 {
					report_err!(
						&mut reports,
						token.source(),
						"Missing Section Spacing".into(),
						span(
							name.range(),
							"Sections require at least one whitespace before the section's name"
								.into()
						),
						help(format!(
							"Add a space before `{}`",
							section_name.fg(state.parser.colors().highlight)
						))
					);
					return reports;
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
			.current(SectionStyle::key())
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

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			sems.add(matches.get(1).unwrap().range(), tokens.section_heading);
			if let Some(reference) = matches.get(2) {
				sems.add(
					reference.start() - 1..reference.end() + 1,
					tokens.section_reference,
				);
			}
			if let Some(kind) = matches.get(3) {
				sems.add(kind.range(), tokens.section_kind);
			}
			sems.add(matches.get(5).unwrap().range(), tokens.section_name);
		}

		reports
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];

		bindings.push((
			"push".to_string(),
			lua.create_function(
				|_,
				 (title, depth, kind, reference): (
					String,
					usize,
					Option<String>,
					Option<String>,
				)| {
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
								cause: Arc::new(mlua::Error::external(
									"Unknown section kind specified".to_string(),
								)),
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
								.current(SectionStyle::key())
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
}
