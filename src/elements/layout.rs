use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::parser::Parser;
use crate::parser::parser::ReportColors;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::state::Scope;
use crate::parser::state::State;
use crate::parser::util::process_escaped;
use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use lazy_static::lazy_static;
use mlua::Function;
use mlua::Lua;
use regex::Captures;
use regex::Match;
use regex::Regex;
use regex::RegexBuilder;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutToken {
	Begin,
	Next,
	End,
}

/// Represents the type of a layout
pub trait LayoutType: core::fmt::Debug {
	/// Name of the layout
	fn name(&self) -> &'static str;

	/// Parses layout properties
	fn parse_properties(&self, properties: &str) -> Result<Option<Box<dyn Any>>, String>;

	/// Expected number of blocks
	fn expects(&self) -> Range<usize>;

	/// Compile layout
	fn compile(
		&self,
		token: LayoutToken,
		id: usize,
		properties: &Option<Box<dyn Any>>,
		compiler: &Compiler,
		document: &dyn Document,
	) -> Result<String, String>;
}

mod default_layouts {
	use std::any::Any;

	use crate::parser::util::Property;
	use crate::parser::util::PropertyParser;

	use super::*;

	#[derive(Debug)]
	pub struct Centered(PropertyParser);

	impl Default for Centered {
		fn default() -> Self {
			let mut properties = HashMap::new();
			properties.insert(
				"style".to_string(),
				Property::new(
					true,
					"Additional style for the split".to_string(),
					Some("".to_string()),
				),
			);

			Self(PropertyParser { properties })
		}
	}

	impl LayoutType for Centered {
		fn name(&self) -> &'static str { "Centered" }

		fn expects(&self) -> Range<usize> { 1..1 }

		fn parse_properties(&self, properties: &str) -> Result<Option<Box<dyn Any>>, String> {
			let props = if properties.is_empty() {
				self.0.default()
			} else {
				self.0.parse(properties)
			}
			.map_err(|err| {
				format!(
					"Failed to parse properties for layout {}: {err}",
					self.name()
				)
			})?;

			let style = props
				.get("style", |_, value| -> Result<String, ()> {
					Ok(value.clone())
				})
				.map_err(|err| format!("Failed to parse style: {err:#?}"))
				.map(|(_, value)| value)?;

			Ok(Some(Box::new(style)))
		}

		fn compile(
			&self,
			token: LayoutToken,
			_id: usize,
			properties: &Option<Box<dyn Any>>,
			compiler: &Compiler,
			_document: &dyn Document,
		) -> Result<String, String> {
			match compiler.target() {
				Target::HTML => {
					let style = match properties
						.as_ref()
						.unwrap()
						.downcast_ref::<String>()
						.unwrap()
						.as_str()
					{
						"" => "".to_string(),
						str => format!(r#" style={}"#, Compiler::sanitize(compiler.target(), str)),
					};
					match token {
						LayoutToken::Begin => Ok(format!(r#"<div class="centered"{style}>"#)),
						LayoutToken::Next => panic!(),
						LayoutToken::End => Ok(r#"</div>"#.to_string()),
					}
				}
				_ => todo!(""),
			}
		}
	}

	#[derive(Debug)]
	pub struct Split(PropertyParser);

	impl Default for Split {
		fn default() -> Self {
			let mut properties = HashMap::new();
			properties.insert(
				"style".to_string(),
				Property::new(
					true,
					"Additional style for the split".to_string(),
					Some("".to_string()),
				),
			);

			Self(PropertyParser { properties })
		}
	}

	impl LayoutType for Split {
		fn name(&self) -> &'static str { "Split" }

		fn expects(&self) -> Range<usize> { 2..usize::MAX }

		fn parse_properties(&self, properties: &str) -> Result<Option<Box<dyn Any>>, String> {
			let props = if properties.is_empty() {
				self.0.default()
			} else {
				self.0.parse(properties)
			}
			.map_err(|err| {
				format!(
					"Failed to parse properties for layout {}: {err}",
					self.name()
				)
			})?;

			let style = props
				.get("style", |_, value| -> Result<String, ()> {
					Ok(value.clone())
				})
				.map_err(|err| format!("Failed to parse style: {err:#?}"))
				.map(|(_, value)| value)?;

			Ok(Some(Box::new(style)))
		}

		fn compile(
			&self,
			token: LayoutToken,
			_id: usize,
			properties: &Option<Box<dyn Any>>,
			compiler: &Compiler,
			_document: &dyn Document,
		) -> Result<String, String> {
			match compiler.target() {
				Target::HTML => {
					let style = match properties
						.as_ref()
						.unwrap()
						.downcast_ref::<String>()
						.unwrap()
						.as_str()
					{
						"" => "".to_string(),
						str => format!(r#" style={}"#, Compiler::sanitize(compiler.target(), str)),
					};
					match token {
						LayoutToken::Begin => Ok(format!(
							r#"<div class="split-container"><div class="split"{style}>"#
						)),
						LayoutToken::Next => Ok(format!(r#"</div><div class="split"{style}>"#)),
						LayoutToken::End => Ok(r#"</div></div>"#.to_string()),
					}
				}
				_ => todo!(""),
			}
		}
	}
}

#[derive(Debug)]
struct Layout {
	pub(self) location: Token,
	pub(self) layout: Rc<dyn LayoutType>,
	pub(self) id: usize,
	pub(self) token: LayoutToken,
	pub(self) properties: Option<Box<dyn Any>>,
}

impl Element for Layout {
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Block }
	fn element_name(&self) -> &'static str { "Layout" }
	fn to_string(&self) -> String { format!("{self:#?}") }
	fn compile(&self, compiler: &Compiler, document: &dyn Document) -> Result<String, String> {
		self.layout
			.compile(self.token, self.id, &self.properties, compiler, document)
	}
}

struct LayoutState {
	/// The layout stack
	pub(self) stack: Vec<(Vec<Token>, Rc<dyn LayoutType>)>,
}

impl State for LayoutState {
	fn scope(&self) -> Scope { Scope::DOCUMENT }

	fn on_remove<'a>(
		&self,
		parser: &dyn Parser,
		document: &dyn Document,
	) -> Vec<Report<'a, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let doc_borrow = document.content().borrow();
		let at = doc_borrow.last().unwrap().location();

		for (tokens, layout_type) in &self.stack {
			let start = tokens.first().unwrap();
			reports.push(
				Report::build(ReportKind::Error, start.source(), start.start())
					.with_message("Unterminated Layout")
					.with_label(
						Label::new((start.source(), start.range.start + 1..start.range.end))
							.with_order(1)
							.with_message(format!(
								"Layout {} stars here",
								layout_type.name().fg(parser.colors().info)
							))
							.with_color(parser.colors().error),
					)
					.with_label(
						Label::new((at.source(), at.range.clone()))
							.with_order(2)
							.with_message("Document ends here".to_string())
							.with_color(parser.colors().error),
					)
					.finish(),
			);
		}

		return reports;
	}
}

pub struct LayoutRule {
	re: [Regex; 3],
	layouts: HashMap<String, Rc<dyn LayoutType>>,
}

impl LayoutRule {
	pub fn new() -> Self {
		let mut layouts: HashMap<String, Rc<dyn LayoutType>> = HashMap::new();

		let layout_centered = default_layouts::Centered::default();
		layouts.insert(layout_centered.name().to_string(), Rc::new(layout_centered));

		let layout_split = default_layouts::Split::default();
		layouts.insert(layout_split.name().to_string(), Rc::new(layout_split));

		Self {
			re: [
				RegexBuilder::new(
					r"(?:^|\n)(?:[^\S\n]*)#\+LAYOUT_BEGIN(?:\[((?:\\.|[^\\\\])*?)\])?(.*)",
				)
				.multi_line(true)
				.build()
				.unwrap(),
				RegexBuilder::new(
					r"(?:^|\n)(?:[^\S\n]*)#\+LAYOUT_NEXT(?:\[((?:\\.|[^\\\\])*?)\])?$",
				)
				.multi_line(true)
				.build()
				.unwrap(),
				RegexBuilder::new(
					r"(?:^|\n)(?:[^\S\n]*)#\+LAYOUT_END(?:\[((?:\\.|[^\\\\])*?)\])?$",
				)
				.multi_line(true)
				.build()
				.unwrap(),
			],
			layouts,
		}
	}

	pub fn parse_properties<'a>(
		colors: &ReportColors,
		token: &Token,
		layout_type: Rc<dyn LayoutType>,
		properties: Option<Match>,
	) -> Result<Option<Box<dyn Any>>, Report<'a, (Rc<dyn Source>, Range<usize>)>> {
		match properties {
			None => match layout_type.parse_properties("") {
				Ok(props) => Ok(props),
				Err(err) => Err(
					Report::build(ReportKind::Error, token.source(), token.start())
						.with_message("Unable to parse layout properties")
						.with_label(
							Label::new((token.source(), token.range.clone()))
								.with_message(err)
								.with_color(colors.error),
						)
						.finish(),
				),
			},
			Some(props) => {
				let trimmed = props.as_str().trim_start().trim_end();
				let content = process_escaped('\\', "]", trimmed);
				match layout_type.parse_properties(content.as_str()) {
					Ok(props) => Ok(props),
					Err(err) => {
						Err(
							Report::build(ReportKind::Error, token.source(), props.start())
								.with_message("Unable to parse layout properties")
								.with_label(
									Label::new((token.source(), props.range()))
										.with_message(err)
										.with_color(colors.error),
								)
								.finish(),
						)
					}
				}
			}
		}
	}
}

lazy_static! {
	static ref STATE_NAME: String = "elements.layout".to_string();
}

impl RegexRule for LayoutRule {
	fn name(&self) -> &'static str { "Layout" }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn on_regex_match(
		&self,
		index: usize,
		parser: &dyn Parser,
		document: &dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report<(Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let query = parser.state().query(&STATE_NAME);
		let state = match query {
			Some(state) => state,
			None => {
				// Insert as a new state
				match parser.state_mut().insert(
					STATE_NAME.clone(),
					Rc::new(RefCell::new(LayoutState { stack: vec![] })),
				) {
					Err(_) => panic!("Unknown error"),
					Ok(state) => state,
				}
			}
		};

		if index == 0
		// BEGIN_LAYOUT
		{
			match matches.get(2) {
				None => {
					reports.push(
						Report::build(ReportKind::Error, token.source(), token.start())
							.with_message("Missing Layout Name")
							.with_label(
								Label::new((token.source(), token.range.clone()))
									.with_message(format!(
										"Missing layout name after `{}`",
										"#+BEGIN_LAYOUT".fg(parser.colors().highlight)
									))
									.with_color(parser.colors().error),
							)
							.finish(),
					);
					return reports;
				}
				Some(name) => {
					let trimmed = name.as_str().trim_start().trim_end();
					if name.as_str().is_empty() || trimmed.is_empty()
					// Empty name
					{
						reports.push(
							Report::build(ReportKind::Error, token.source(), name.start())
								.with_message("Empty Layout Name")
								.with_label(
									Label::new((token.source(), token.range.clone()))
										.with_message(format!(
											"Empty layout name after `{}`",
											"#+BEGIN_LAYOUT".fg(parser.colors().highlight)
										))
										.with_color(parser.colors().error),
								)
								.finish(),
						);
						return reports;
					} else if !name.as_str().chars().next().unwrap().is_whitespace()
					// Missing space
					{
						reports.push(
							Report::build(ReportKind::Error, token.source(), name.start())
								.with_message("Invalid Layout Name")
								.with_label(
									Label::new((token.source(), name.range()))
										.with_message(format!(
											"Missing a space before layout name `{}`",
											name.as_str().fg(parser.colors().highlight)
										))
										.with_color(parser.colors().error),
								)
								.finish(),
						);
						return reports;
					}

					// Get layout
					let layout_type = match self.layouts.get(trimmed) {
						None => {
							reports.push(
								Report::build(ReportKind::Error, token.source(), name.start())
									.with_message("Unknown Layout")
									.with_label(
										Label::new((token.source(), name.range()))
											.with_message(format!(
												"Cannot find layout `{}`",
												trimmed.fg(parser.colors().highlight)
											))
											.with_color(parser.colors().error),
									)
									.finish(),
							);
							return reports;
						}
						Some(layout_type) => layout_type,
					};

					// Parse properties
					let properties = match LayoutRule::parse_properties(
						parser.colors(),
						&token,
						layout_type.clone(),
						matches.get(1),
					) {
						Ok(props) => props,
						Err(rep) => {
							reports.push(rep);
							return reports;
						}
					};

					parser.push(
						document,
						Box::new(Layout {
							location: token.clone(),
							layout: layout_type.clone(),
							id: 0,
							token: LayoutToken::Begin,
							properties,
						}),
					);

					state
						.borrow_mut()
						.downcast_mut::<LayoutState>()
						.map_or_else(
							|| panic!("Invalid state at: `{}`", STATE_NAME.as_str()),
							|s| s.stack.push((vec![token.clone()], layout_type.clone())),
						);
				}
			};
			return reports;
		}

		let (id, token_type, layout_type, properties) = if index == 1
		// LAYOUT_NEXT
		{
			let mut state_borrow = state.borrow_mut();
			let state = state_borrow.downcast_mut::<LayoutState>().unwrap();

			let (tokens, layout_type) = match state.stack.last_mut() {
				None => {
					reports.push(
						Report::build(ReportKind::Error, token.source(), token.start())
							.with_message("Invalid #+LAYOUT_NEXT")
							.with_label(
								Label::new((token.source(), token.range.clone()))
									.with_message("No active layout found".to_string())
									.with_color(parser.colors().error),
							)
							.finish(),
					);
					return reports;
				}
				Some(last) => last,
			};

			if layout_type.expects().end < tokens.len()
			// Too many blocks
			{
				reports.push(
					Report::build(ReportKind::Error, token.source(), token.start())
						.with_message("Unexpected #+LAYOUT_NEXT")
						.with_label(
							Label::new((token.source(), token.range.clone()))
								.with_message(format!(
									"Layout expects a maximum of {} blocks, currently at {}",
									layout_type.expects().end.fg(parser.colors().info),
									tokens.len().fg(parser.colors().info),
								))
								.with_color(parser.colors().error),
						)
						.finish(),
				);
				return reports;
			}

			// Parse properties
			let properties = match LayoutRule::parse_properties(
				parser.colors(),
				&token,
				layout_type.clone(),
				matches.get(1),
			) {
				Ok(props) => props,
				Err(rep) => {
					reports.push(rep);
					return reports;
				}
			};

			tokens.push(token.clone());
			(
				tokens.len() - 1,
				LayoutToken::Next,
				layout_type.clone(),
				properties,
			)
		} else {
			// LAYOUT_END
			let mut state_borrow = state.borrow_mut();
			let state = state_borrow.downcast_mut::<LayoutState>().unwrap();

			let (tokens, layout_type) = match state.stack.last_mut() {
				None => {
					reports.push(
						Report::build(ReportKind::Error, token.source(), token.start())
							.with_message("Invalid #+LAYOUT_END")
							.with_label(
								Label::new((token.source(), token.range.clone()))
									.with_message("No active layout found".to_string())
									.with_color(parser.colors().error),
							)
							.finish(),
					);
					return reports;
				}
				Some(last) => last,
			};

			if layout_type.expects().start > tokens.len()
			// Not enough blocks
			{
				reports.push(
					Report::build(ReportKind::Error, token.source(), token.start())
						.with_message("Unexpected #+LAYOUT_END")
						.with_label(
							Label::new((token.source(), token.range.clone()))
								.with_message(format!(
									"Layout expects a minimum of {} blocks, currently at {}",
									layout_type.expects().start.fg(parser.colors().info),
									tokens.len().fg(parser.colors().info),
								))
								.with_color(parser.colors().error),
						)
						.finish(),
				);
				return reports;
			}

			// Parse properties
			let properties = match LayoutRule::parse_properties(
				parser.colors(),
				&token,
				layout_type.clone(),
				matches.get(1),
			) {
				Ok(props) => props,
				Err(rep) => {
					reports.push(rep);
					return reports;
				}
			};

			let layout_type = layout_type.clone();
			let id = tokens.len();
			state.stack.pop();
			(id, LayoutToken::End, layout_type, properties)
		};

		parser.push(
			document,
			Box::new(Layout {
				location: token,
				layout: layout_type,
				id,
				token: token_type,
				properties,
			}),
		);

		return reports;
	}

	// TODO
	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Option<Vec<(String, Function<'lua>)>> { None }
}

#[cfg(test)]
mod tests {
	use crate::elements::paragraph::Paragraph;
	use crate::elements::text::Text;
	use crate::parser::langparser::LangParser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;

	use super::*;

	#[test]
	fn parser() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
#+LAYOUT_BEGIN[style=A] Split
	A
	#+LAYOUT_BEGIN[style=B] Centered
		B
	#+LAYOUT_END
#+LAYOUT_NEXT[style=C]
	C
	#+LAYOUT_BEGIN[style=D] Split
		D
	#+LAYOUT_NEXT[style=E]
		E
	#+LAYOUT_END
#+LAYOUT_END
"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let doc = parser.parse(source, None);

		validate_document!(doc.content().borrow(), 0,
			Layout { token == LayoutToken::Begin, id == 0 };
			Paragraph {
				Text { content == "A" };
			};
			Layout { token == LayoutToken::Begin, id == 0 };
			Paragraph {
				Text { content == "B" };
			};
			Layout { token == LayoutToken::End, id == 1 };
			Layout { token == LayoutToken::Next, id == 1 };
			Paragraph {
				Text { content == "C" };
			};
			Layout { token == LayoutToken::Begin, id == 0 };
			Paragraph {
				Text { content == "D" };
			};
			Layout { token == LayoutToken::Next, id == 1 };
			Paragraph {
				Text { content == "E" };
			};
			Layout { token == LayoutToken::End, id == 2 };
			Layout { token == LayoutToken::End, id == 2 };
		);
	}
}
