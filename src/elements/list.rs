use std::{any::Any, cell::Ref, ops::Range, rc::Rc};

use crate::{compiler::compiler::{Compiler, Target}, document::{document::{Document, DocumentAccessors}, element::{ElemKind, Element}}, parser::{parser::Parser, rule::Rule, source::{Cursor, Source, Token, VirtualSource}}};
use ariadne::{Label, Report, ReportKind};
use mlua::{Function, Lua};
use regex::Regex;

use super::paragraph::Paragraph;

#[derive(Debug)]
pub struct ListEntry {
	location: Token,
	numbering: Vec<(bool, usize)>,
	content: Vec<Box<dyn Element>>,

	// TODO bullet_maker : FnMut<...>
}

impl ListEntry {
	pub fn new(location: Token, numbering: Vec<(bool, usize)>, content: Vec<Box<dyn Element>>) -> Self {
		Self { location, numbering, content }
	}
}

#[derive(Debug)]
pub struct List
{
	location: Token,
	entries: Vec<ListEntry>
}

impl List
{
	pub fn new(location: Token) -> Self
	{
		Self
		{
			location,
			entries: Vec::new()
		}
	}

	pub fn push(&mut self, entry: ListEntry)
	{
		self.location.range = self.location.start()..entry.location.end();
		self.entries.push(entry);
	}
}

impl Element for List
{
    fn location(&self) -> &Token { &self.location }

    fn kind(&self) -> ElemKind { ElemKind::Block }

    fn element_name(&self) -> &'static str { "List" }

    fn to_string(&self) -> String { format!("{self:#?}") }

    fn compile(&self, compiler: &Compiler, document: &dyn Document) -> Result<String, String> {
		match compiler.target()
		{
			Target::HTML => {
				let mut result = String::new();

				//TODO: Do something about indexing
				let mut current_list: Vec<bool> = vec![];
				let mut match_stack = |result: &mut String, target: &Vec<(bool, usize)>| {

					// Find index after which current_list and target differ
					let mut match_idx = 0usize;
					for i in 0..current_list.len()
					{
						if i >= target.len() || current_list[i] != target[i].0 { break }
						else { match_idx = i+1; }
					}

					// Close until same match
					for _ in match_idx..current_list.len()
					{
						result.push_str(["</ul>", "</ol>"][current_list.pop().unwrap() as usize]);
					}

					// Open
					for i in match_idx..target.len()
					{
						result.push_str(["<ul>", "<ol>"][target[i].0 as usize]);
						current_list.push(target[i].0);
					}
				};

				match self.entries.iter()
					.try_for_each(|ent|
					{
						match_stack(&mut result, &ent.numbering);
						result.push_str("<li>");
						match ent.content.iter().enumerate()
							.try_for_each(|(idx, elem)| { 
								match elem.compile(compiler, document) {
									Err(e) => Err(e),
									Ok(s) => { result.push_str(s.as_str()); Ok(()) }
								}
							})
						{
							Err(e) => Err(e),
							_ => {
								result.push_str("</li>");
								Ok(())
							}
						}
					})
				{
					Err(e) => return Err(e),
					_ => {}
				}
				match_stack(&mut result, &Vec::<(bool, usize)>::new());

				Ok(result)
			}
			Target::LATEX => Err("Unimplemented compiler".to_string())
		}
    }
}

/*
impl Element for ListEntry
{
	fn location(&self) -> &Token { &self.location }
	fn kind(&self) -> ElemKind { ElemKind::Inline }
	fn element_name(&self) -> &'static str { "List" }
	fn to_string(&self) -> String { format!("{self:#?}") }
	fn compile(&self, compiler: &Compiler) -> Result<String, String> {
		lazy_static! {
			static ref STATE_NAME : &'static str = "list.state";
			static ref LIST_OPEN : [&'static str; 2] = ["<ul>", "<ol>"];
			static ref LIST_CLOSE : [&'static str; 2] = ["</ul>", "</ol>"];
		}

		// TODO: State.shouldpreserve?
		// Called upon every element
		//let state = compiler.get_state_mut::<ListState, _>(*STATE_NAME)
		//.or_else(|| {
		//	compiler.insert_state(STATE_NAME.to_string(), Box::new(ListState(Vec::new())) as Box<dyn Any>);
		//	compiler.get_state_mut::<ListState, _>(*STATE_NAME)
		//}).unwrap();

		match compiler.target()
		{
			Target::HTML => {
				let mut result = String::new();

				//TODO: Do something about indexing
				//&self.numbering.iter()
				//	.zip(&state.0)
				//	.for_each(|((wants_numbered, _), is_numbered)|
				//	{
				//		
				//	});

				result.push_str("<li>");
				match self.content.iter()
					.try_for_each(|ent| match ent.compile(compiler) {
						Err(e) => Err(e),
						Ok(s) => Ok(result.push_str(s.as_str())),
					})
				{
					Err(e) => return Err(e),
					_ => {}
				}
				result.push_str("</li>");
				//result.push_str(LIST_OPEN[self.numbered as usize]);
				//self.entries.iter()
				//	.for_each(|(_index, entry)|
				//		result.push_str(format!("<li>{}</li>", compiler.compile(entry)).as_str()));
				//result.push_str(LIST_CLOSE[self.numbered as usize]);
				Ok(result)
			}
			Target::LATEX => Err("Unimplemented compiler".to_string())
		}
	}
}
*/

pub struct ListRule
{
	start_re: Regex,
	continue_re: Regex
}

impl ListRule {
	pub fn new() -> Self {
		Self {
			start_re: Regex::new(r"(?:^|\n)(?:[^\S\r\n]+)([*-]+).*").unwrap(),
			continue_re: Regex::new(r"(?:^|\n)([^\S\r\n]+).*").unwrap(),
		}

	}

	fn parse_depth(depth: &str, document: &dyn Document) -> Vec<(bool, usize)>
	{
		let mut parsed = vec![];
		// FIXME: Previous iteration used to recursively retrieve the list indent
		let prev_entry = document.last_element::<List>()
			.and_then(|list| Ref::filter_map(list, |m| m.entries.last() ).ok() )
			.and_then(|entry| Ref::filter_map(entry, |e| Some(&e.numbering)).ok() );

		let mut continue_match = true;
		depth.chars().enumerate().for_each(|(idx, c)|
		{
			let number = prev_entry.as_ref()
				.and_then(|v| {
					if !continue_match { return None }
					let numbered = c == '-';

					match v.get(idx)
					{
						None => None,
						Some((prev_numbered, prev_idx)) => {
							if *prev_numbered != numbered { continue_match = false; None } // New depth
							else if idx+1 == v.len() { Some(prev_idx+1) } // Increase from previous
							else { Some(*prev_idx) } // Do nothing
						}
					}
				})
				.or(Some(0usize))
				.unwrap();

			match c
			{
				'*' => parsed.push((false, number)),
				'-' => parsed.push((true, number)),
				_ => panic!("Unimplemented")
			}
		});

		return parsed;
	}
}

impl Rule for ListRule
{
	fn name(&self) -> &'static str { "List" }

	fn next_match(&self, cursor: &Cursor) -> Option<(usize, Box<dyn Any>)> {
		self.start_re.find_at(cursor.source.content(), cursor.pos)
			.map_or(None,
			|m| Some((m.start(), Box::new([false;0]) as Box<dyn Any>)) )
	}

	fn on_match<'a>(&self, parser: &dyn Parser, document: &'a dyn Document<'a>, cursor: Cursor, _match_data: Option<Box<dyn Any>>)
		-> (Cursor, Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>) {
		let mut reports = vec![];
		let content = cursor.source.content();
		let (end_cursor, numbering, source) = match self.start_re.captures_at(content, cursor.pos) {
			None => panic!("Unknown error"),
			Some(caps) => {
				let mut end_pos = caps.get(0).unwrap().end();

				let mut spacing = None; // Spacing used to continue list entry
				loop {
					// If another entry starts on the next line, don't continue matching
					match self.next_match(&cursor.at(end_pos))
					{
						Some((pos, _)) => {
							if pos == end_pos { break }
						}
						None => {},
					}

					// Continue matching as current entry
					match self.continue_re.captures_at(content, end_pos) {
						None => break,
						Some(continue_caps) => {
							if continue_caps.get(0).unwrap().start() != end_pos { break }

							// Get the spacing
							let cap_spacing = continue_caps.get(1).unwrap();
							match &spacing {
								None => spacing = Some(cap_spacing.range()),
								Some(spacing) => 'some: {
									if content[cap_spacing.range()] == content[spacing.clone()] { break 'some }

									reports.push(
										Report::build(ReportKind::Warning, cursor.source.clone(), continue_caps.get(1).unwrap().start())
										.with_message("Invalid list entry spacing")
										.with_label(
											Label::new((cursor.source.clone(), cap_spacing.range()))
											.with_message("Spacing for list entries must match")
											.with_color(parser.colors().warning))
										.with_label(
											Label::new((cursor.source.clone(), spacing.clone()))
											.with_message("Previous spacing")
											.with_color(parser.colors().warning))
										.finish());
								},
							}
							end_pos = continue_caps.get(0).unwrap().end();
						}
					}
				}

				let start_pos = caps.get(1).unwrap().end();
				let source = VirtualSource::new(
					Token::new(start_pos..end_pos, cursor.source.clone()),
					"List Entry".to_string(),
					content.as_str()[start_pos..end_pos].to_string(),
				);

				(cursor.at(end_pos),
				ListRule::parse_depth(caps.get(1).unwrap().as_str(), document),
				source)
			},
		};

        let parsed_entry = parser.parse(Rc::new(source), Some(document));
		let mut parsed_paragraph = parsed_entry.last_element_mut::<Paragraph>().unwrap(); // Extract content from paragraph
		let entry = ListEntry::new(
			Token::new(cursor.pos..end_cursor.pos, cursor.source.clone()),
			numbering,
			std::mem::replace(&mut parsed_paragraph.content, Vec::new())
		);

		// Ger previous list, if none insert a new list
		let mut list = match document.last_element_mut::<List>()
		{
			Some(last) => last,
			None => {
				parser.push(document,
					Box::new(List::new(
							Token::new(cursor.pos..end_cursor.pos, cursor.source.clone()))));
				document.last_element_mut::<List>().unwrap()
			}
		};
		list.push(entry);

		(end_cursor, reports)
	}

	// TODO
	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Vec<(String, Function<'lua>)> { vec![] }
}
