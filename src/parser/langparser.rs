use std::{cell::{RefCell, RefMut}, collections::{HashMap, HashSet}, ops::Range, rc::Rc};

use ariadne::{Label, Report};

use crate::{document::{document::Document, element::{ElemKind, Element}}, elements::{paragraph::Paragraph, registrar::register, text::Text}, lua::kernel::{Kernel, KernelHolder}, parser::source::{SourceFile, VirtualSource}};

use super::{parser::{Parser, ReportColors}, rule::Rule, source::{Cursor, Source, Token}, state::StateHolder, util};

/// Parser for the language
#[derive(Debug)]
pub struct LangParser
{
	rules: Vec<Box<dyn Rule>>,
	colors: ReportColors,

	// Parser state
	pub err_flag: RefCell<bool>,
	pub state: RefCell<StateHolder>,
	pub kernels: RefCell<HashMap<String, Kernel>>,
}

impl LangParser
{
	pub fn default() -> Self
	{
		let mut s = Self {
			rules: vec![],
			colors: ReportColors::with_colors(),
			err_flag: RefCell::new(false),
			state: RefCell::new(StateHolder::new()),
			kernels: RefCell::new(HashMap::new()),
		};
		register(&mut s);

		s.kernels.borrow_mut()
			.insert("main".to_string(), Kernel::new(&s));
		s
	}

	fn handle_reports<'a>(&self, _source: Rc<dyn Source>, reports: Vec<Report<'a, (Rc<dyn Source>, Range<usize>)>>)
	{
		for mut report in reports
		{
			let mut sources: HashSet<Rc<dyn Source>> = HashSet::new();
			fn recurse_source(sources: &mut HashSet<Rc<dyn Source>>, source: Rc<dyn Source>) {
				sources.insert(source.clone());
				match source.location()
				{
					Some(parent) => {
						let parent_source = parent.source();
						if sources.get(&parent_source).is_none()
						{
							recurse_source(sources, parent_source);
						}
					}
					None => {}
				}
			}

			report.labels.iter().for_each(|label| {
				recurse_source(&mut sources, label.span.0.clone());
			});

			let cache = sources.iter()
				.map(|source| (source.clone(), source.content().clone()))
				.collect::<Vec<(Rc<dyn Source>, String)>>();

			cache.iter()
				.for_each(|(source, _)| {
					if let Some (location) = source.location()
					{
						if let Some(_s) = source.downcast_ref::<SourceFile>()
						{
							report.labels.push(
								Label::new((location.source(), location.start()+1 .. location.end()))
								.with_message("In file included from here")
								.with_order(-1)
							);
						};

						if let Some(_s) = source.downcast_ref::<VirtualSource>()
						{
							let start = location.start() + (location.source().content().as_bytes()[location.start()] == '\n' as u8)
								.then_some(1)
								.unwrap_or(0);
							report.labels.push(
								Label::new((location.source(), start .. location.end()))
								.with_message("In evaluation of")
								.with_order(-1)
							);
						};
					}
				});
			report.eprint(ariadne::sources(cache)).unwrap()
		}
	}
}

impl Parser for LangParser
{
	fn colors(&self) -> &ReportColors { &self.colors }

	fn rules(&self) -> &Vec<Box<dyn Rule>> { &self.rules }
	fn add_rule(&mut self, rule: Box<dyn Rule>, after: Option<&'static str>)
	{
		// Error on duplicate rule
		let rule_name = (*rule).name();
		self.rules.iter().for_each(|rule| {
			if (*rule).name() != rule_name { return; }
		
			panic!("Attempted to introduce duplicate rule: `{rule_name}`");
		});

		match after
		{
			Some(name) => {
				let before = self.rules.iter()
					.enumerate()
					.find(|(_pos, r)| (r).name() == name);

				match before
				{
					Some((pos, _)) => self.rules.insert(pos+1, rule),
					_ => panic!("Unable to find rule named `{name}`, to insert rule `{}` after it", rule.name())
				}
			}
			_ => self.rules.push(rule)
		}
	}

	fn state(&self) -> std::cell::Ref<'_, StateHolder> { self.state.borrow() }
	fn state_mut(&self) -> std::cell::RefMut<'_, StateHolder> { self.state.borrow_mut() }

	/// Add an [`Element`] to the [`Document`]
	fn push<'a>(&self, doc: &'a Document<'a>, elem: Box<dyn Element>)
	{
		if elem.kind() == ElemKind::Inline || elem.kind() == ElemKind::Invisible
		{
			let mut paragraph = doc.last_element_mut::<Paragraph>(false)
				.or_else(|| {
					doc.push(Box::new(Paragraph::new(elem.location().clone())));
					doc.last_element_mut::<Paragraph>(false)
				}).unwrap();

			paragraph.push(elem);
		}
		else
		{
			// Process paragraph events
			if doc.last_element_mut::<Paragraph>(false)
				.is_some_and(|_| true)
			{
				self.handle_reports(doc.source(),
				self.state_mut().on_scope_end(self, &doc, super::state::Scope::PARAGRAPH));
			}

			doc.push(elem);
		}
	}

	fn parse<'a>(&self, source: Rc<dyn Source>, parent: Option<&'a Document<'a>>) -> Document<'a>
	{
		let doc = Document::new(source.clone(), parent);
		let mut matches = Vec::new();
		for _ in 0..self.rules.len() {
			matches.push((0usize, None));
		}

		let content = source.content();
		let mut cursor = Cursor::new(0usize, doc.source()); // Cursor in file

		if parent.is_some() // Terminate parent's paragraph state
		{
			self.handle_reports(parent.as_ref().unwrap().source(),
			self.state_mut().on_scope_end(self, parent.as_ref().unwrap(), super::state::Scope::PARAGRAPH));
		}
	
		loop
		{
			let (rule_pos, rule, match_data) = self.update_matches(&cursor, &mut matches);

			// Unmatched content
			let text_content = util::process_text(&doc, &content.as_str()[cursor.pos..rule_pos.pos]);
			if !text_content.is_empty()
			{
				self.push(&doc, Box::new(Text::new(
					Token::new(cursor.pos..rule_pos.pos, source.clone()),
					text_content
				)));
			}

			if let Some(rule) = rule
			{

				// Rule callback
				let (new_cursor, reports) = (*rule).on_match(self, &doc, rule_pos, match_data);

				self.handle_reports(doc.source(), reports);

				// Advance
				cursor = new_cursor;
			}
			else // No rules left
			{
				break;
			}
		}

		// State
		self.handle_reports(doc.source(),
			self.state_mut().on_scope_end(self, &doc, super::state::Scope::DOCUMENT));
		
		return doc;
	}

	fn parse_into<'a>(&self, source: Rc<dyn Source>, document: &'a Document<'a>)
	{
		let mut matches = Vec::new();
		for _ in 0..self.rules.len() {
			matches.push((0usize, None));
		}

		let content = source.content();
		let mut cursor = Cursor::new(0usize, source.clone());

		loop
		{
			let (rule_pos, rule, match_data) = self.update_matches(&cursor, &mut matches);

			// Unmatched content
			let text_content = util::process_text(&document, &content.as_str()[cursor.pos..rule_pos.pos]);
			if !text_content.is_empty()
			{
				self.push(&document, Box::new(Text::new(
							Token::new(cursor.pos..rule_pos.pos, source.clone()),
							text_content
				)));
			}

			if let Some(rule) = rule
			{
				// Rule callback
				let (new_cursor, reports) = (*rule).on_match(self, &document, rule_pos, match_data);

				self.handle_reports(document.source(), reports);

				// Advance
				cursor = new_cursor;
			}
			else // No rules left
			{
				break;
			}
		}

		// State
		//self.handle_reports(source.clone(),
		//	self.state_mut().on_scope_end(&self, &document, super::state::Scope::DOCUMENT));
		
		//return doc;
	}
}

impl KernelHolder for LangParser
{
    fn get_kernel(&self, name: &str)
		-> Option<RefMut<'_, Kernel>> {
		RefMut::filter_map(self.kernels.borrow_mut(),
		|map| map.get_mut(name)).ok()
    }

    fn insert_kernel(&self, name: String, kernel: Kernel)
		-> RefMut<'_, Kernel> {
			//TODO do not get
		self.kernels.borrow_mut()
			.insert(name.clone(), kernel);
		self.get_kernel(name.as_str()).unwrap()
    }
}
