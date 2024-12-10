use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::elements::section::elem::Section;
use crate::elements::section::rule::section_kind;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct Toc {
	pub(crate) location: Token,
	pub(crate) title: Option<String>,
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

		if sections.is_empty() {
			return Ok("".into());
		}

		match compiler.target() {
			HTML => {
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
							"<li style=\"list-style-type:none\"><a href=\"#{}\">{}</a></li>",
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
