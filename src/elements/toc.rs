use regex::Captures;
use regex::Regex;
use regex::RegexBuilder;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::elements::section::section_kind;
use crate::elements::section::Section;
use crate::lsp::semantic::Semantics;
use crate::lua::kernel::CTX;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::reports::Report;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;

#[derive(Debug)]
struct Toc {
	pub(self) location: Token,
	pub(self) title: Option<String>,
}

impl Element for Toc {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Block }
	fn element_name(&self) -> &'static str { "Toc" }
	fn compile(
		&self,
		compiler: &Compiler,
		document: &dyn Document,
		_cursor: usize,
	) -> Result<String, String> {
		let mut result = String::new();
		let mut sections: Vec<(&Section, usize)> = vec![];
		// Find last section with given depth
		fn last_matching(depth: usize, sections: &Vec<(&Section, usize)>) -> Option<usize> {
			for (idx, (section, _number)) in sections.iter().rev().enumerate() {
				if section.depth < depth {
					return None;
				} else if section.depth == depth {
					return Some(sections.len() - idx - 1);
				}
			}

			None
		}
		let content_borrow = document.content().borrow();
		for elem in content_borrow.iter() {
			if let Some(section) = elem.downcast_ref::<Section>() {
				if section.kind & section_kind::NO_TOC != 0 {
					continue;
				}
				let last = last_matching(section.depth, &sections);
				if let Some(last) = last {
					if sections[last].0.kind & section_kind::NO_NUMBER != 0 {
						sections.push((section, sections[last].1));
					} else {
						sections.push((section, sections[last].1 + 1))
					}
				} else {
					sections.push((section, 1));
				}
			}
		}

		match compiler.target() {
			Target::HTML => {
				let match_depth = |current: usize, target: usize| -> String {
					let mut result = String::new();
					for _ in current..target {
						result += "<ol>";
					}
					for _ in target..current {
						result += "</ol>";
					}
					result
				};
				result += "<div class=\"toc\">";
				result += format!(
					"<span>{}</span>",
					Compiler::sanitize(
						compiler.target(),
						self.title.as_ref().unwrap_or(&String::new())
					)
				)
				.as_str();
				let mut current_depth = 0;
				for (section, number) in sections {
					result += match_depth(current_depth, section.depth).as_str();
					if section.kind & section_kind::NO_NUMBER != 0 {
						result += format!(
							"<li><a href=\"#{}\">{}</a></li>",
							Compiler::refname(compiler.target(), section.title.as_str()),
							Compiler::sanitize(compiler.target(), section.title.as_str())
						)
						.as_str();
					} else {
						result += format!(
							"<li value=\"{number}\"><a href=\"#{}\">{}</a></li>",
							Compiler::refname(compiler.target(), section.title.as_str()),
							Compiler::sanitize(compiler.target(), section.title.as_str())
						)
						.as_str();
					}

					current_depth = section.depth;
				}
				match_depth(current_depth, 0);
				result += "</div>";
			}
			_ => todo!(""),
		}
		Ok(result)
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::toc")]
pub struct TocRule {
	re: [Regex; 1],
}

impl TocRule {
	pub fn new() -> Self {
		Self {
			re: [
				RegexBuilder::new(r"(?:^|\n)(?:[^\S\n]*)#\+TABLE_OF_CONTENT(.*)")
					.multi_line(true)
					.build()
					.unwrap(),
			],
		}
	}
}

impl RegexRule for TocRule {
	fn name(&self) -> &'static str { "Toc" }

	fn previous(&self) -> Option<&'static str> { Some("Layout") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn enabled(&self, mode: &ParseMode, _id: usize) -> bool { !mode.paragraph_only }

	fn on_regex_match(
		&self,
		_index: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		let name = matches.get(1).unwrap().as_str().trim_start().trim_end();

		state.push(
			document,
			Box::new(Toc {
				location: token.clone(),
				title: (!name.is_empty()).then_some(name.to_string()),
			}),
		);

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			let start = matches
				.get(0)
				.map(|m| m.start() + token.source().content()[m.start()..].find('#').unwrap())
				.unwrap();
			sems.add(start..start + 2, tokens.toc_sep);
			sems.add(
				start + 2..start + 2 + "TABLE_OF_CONTENT".len(),
				tokens.toc_token,
			);
			sems.add(matches.get(1).unwrap().range(), tokens.toc_title);
		}

		reports
	}

	fn register_bindings<'lua>(&self, lua: &'lua mlua::Lua) -> Vec<(String, mlua::Function<'lua>)> {
		let mut bindings = vec![];
		bindings.push((
			"push".to_string(),
			lua.create_function(|_, title: Option<String>| {
				CTX.with_borrow(|ctx| {
					ctx.as_ref().map(|ctx| {
						ctx.state.push(
							ctx.document,
							Box::new(Toc {
								location: ctx.location.clone(),
								title,
							}),
						)
					});
				});
				Ok(())
			})
			.unwrap(),
		));
		bindings
	}
}

#[cfg(test)]
mod tests {
	use std::rc::Rc;

	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;
	use crate::validate_semantics;

	use super::*;

	#[test]
	fn parser() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
#+TABLE_OF_CONTENT TOC
# Section1
## SubSection
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
			Toc { title == Some("TOC".to_string()) };
			Section;
			Section;
		);
	}

	#[test]
	fn lua() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
%<nml.toc.push("TOC")>%
%<nml.toc.push()>%
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
			Toc { title == Some("TOC".to_string()) };
			Toc { title == Option::<String>::None };
		);
	}

	#[test]
	fn semantic() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
#+TABLE_OF_CONTENT TOC
		"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (_, state) = parser.parse(
			ParserState::new_with_semantics(&parser, None),
			source.clone(),
			None,
			ParseMode::default(),
		);
		validate_semantics!(state, source.clone(), 0,
			toc_sep { delta_line == 1, delta_start == 0, length == 2 };
			toc_token { delta_line == 0, delta_start == 2, length == 16 };
			toc_title { delta_line == 0, delta_start == 16, length == 4 };
		);
	}
}
