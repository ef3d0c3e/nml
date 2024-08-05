use std::any::Any;
use std::cell::Ref;
use std::cell::RefCell;
use std::cell::RefMut;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

use ariadne::Label;
use ariadne::Report;

use crate::document::customstyle::CustomStyle;
use crate::document::customstyle::CustomStyleHolder;
use crate::document::document::Document;
use crate::document::document::DocumentAccessors;
use crate::document::element::ContainerElement;
use crate::document::element::DocumentEnd;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::document::langdocument::LangDocument;
use crate::document::layout::LayoutHolder;
use crate::document::layout::LayoutType;
use crate::document::style::ElementStyle;
use crate::document::style::StyleHolder;
use crate::elements::paragraph::Paragraph;
use crate::elements::registrar::register;
use crate::elements::text::Text;
use crate::lua::kernel::Kernel;
use crate::lua::kernel::KernelHolder;
use crate::parser::source::SourceFile;
use crate::parser::source::VirtualSource;

use super::parser::Parser;
use super::parser::ParserStrategy;
use super::parser::ReportColors;
use super::rule::Rule;
use super::source::Cursor;
use super::source::Source;
use super::source::Token;
use super::state::StateHolder;
use super::util;

/// Parser for the language
#[derive(Debug)]
pub struct LangParser {
	rules: Vec<Box<dyn Rule>>,
	colors: ReportColors,

	// Parser state
	pub err_flag: RefCell<bool>,
	pub matches: RefCell<Vec<(usize, Option<Box<dyn Any>>)>>,
	
	pub state: RefCell<StateHolder>,
	pub kernels: RefCell<HashMap<String, Kernel>>,
	pub styles: RefCell<HashMap<String, Rc<dyn ElementStyle>>>,
	pub layouts: RefCell<HashMap<String, Rc<dyn LayoutType>>>,
	pub custom_styles: RefCell<HashMap<String, Rc<dyn CustomStyle>>>,
}

impl LangParser {
	pub fn default() -> Self {
		let mut s = Self {
			rules: vec![],
			colors: ReportColors::with_colors(),
			err_flag: RefCell::new(false),
			matches: RefCell::new(Vec::new()),
			state: RefCell::new(StateHolder::new()),
			kernels: RefCell::new(HashMap::new()),
			styles: RefCell::new(HashMap::new()),
			layouts: RefCell::new(HashMap::new()),
			custom_styles: RefCell::new(HashMap::new()),
		};
		// Register rules
		register(&mut s);

		// Register default kernel
		s.kernels
			.borrow_mut()
			.insert("main".to_string(), Kernel::new(&s));

		// Register default styles
		for rule in &s.rules {
			rule.register_styles(&s);
		}

		// Register default layouts
		for rule in &s.rules {
			rule.register_layouts(&s);
		}

		s
	}

	fn handle_reports<'a>(
		&self,
		_source: Rc<dyn Source>,
		reports: Vec<Report<'a, (Rc<dyn Source>, Range<usize>)>>,
	) {
		for mut report in reports {
			let mut sources: HashSet<Rc<dyn Source>> = HashSet::new();
			fn recurse_source(sources: &mut HashSet<Rc<dyn Source>>, source: Rc<dyn Source>) {
				sources.insert(source.clone());
				match source.location() {
					Some(parent) => {
						let parent_source = parent.source();
						if sources.get(&parent_source).is_none() {
							recurse_source(sources, parent_source);
						}
					}
					None => {}
				}
			}

			report.labels.iter().for_each(|label| {
				recurse_source(&mut sources, label.span.0.clone());
			});

			let cache = sources
				.iter()
				.map(|source| (source.clone(), source.content().clone()))
				.collect::<Vec<(Rc<dyn Source>, String)>>();

			cache.iter().for_each(|(source, _)| {
				if let Some(location) = source.location() {
					if let Some(_s) = source.downcast_ref::<SourceFile>() {
						report.labels.push(
							Label::new((location.source(), location.start() + 1..location.end()))
								.with_message("In file included from here")
								.with_order(-1),
						);
					};

					if let Some(_s) = source.downcast_ref::<VirtualSource>() {
						let start = location.start()
							+ (location.source().content().as_bytes()[location.start()]
								== '\n' as u8)
								.then_some(1)
								.unwrap_or(0);
						report.labels.push(
							Label::new((location.source(), start..location.end()))
								.with_message("In evaluation of")
								.with_order(-1),
						);
					};
				}
			});
			report.eprint(ariadne::sources(cache)).unwrap()
		}
	}
}

impl Parser for LangParser {
	fn colors(&self) -> &ReportColors { &self.colors }

	fn rules(&self) -> &Vec<Box<dyn Rule>> { &self.rules }
	fn rules_mut(&mut self) -> &mut Vec<Box<dyn Rule>> { &mut self.rules }

	fn state(&self) -> std::cell::Ref<'_, StateHolder> { self.state.borrow() }
	fn state_mut(&self) -> std::cell::RefMut<'_, StateHolder> { self.state.borrow_mut() }

	fn has_error(&self) -> bool { *self.err_flag.borrow() }

	/// Add an [`Element`] to the [`Document`]
	fn push<'a>(&self, doc: &dyn Document, elem: Box<dyn Element>) {
		if elem.kind() == ElemKind::Inline || elem.kind() == ElemKind::Invisible {
			let mut paragraph = doc
				.last_element_mut::<Paragraph>()
				.or_else(|| {
					doc.push(Box::new(Paragraph {
						location: elem.location().clone(),
						content: Vec::new(),
					}));
					doc.last_element_mut::<Paragraph>()
				})
				.unwrap();

			paragraph.push(elem).unwrap();
		} else {
			// Process paragraph events
			if doc.last_element::<Paragraph>().is_some_and(|_| true) {
				self.handle_reports(
					doc.source(),
					self.state_mut()
						.on_scope_end(self, doc, super::state::Scope::PARAGRAPH),
				);
			}

			doc.push(elem);
		}
	}

	fn parse<'a>(
		&self,
		source: Rc<dyn Source>,
		parent: Option<&'a dyn Document<'a>>,
	) -> Box<dyn Document<'a> + 'a> {
		let doc = LangDocument::new(source.clone(), parent);
		let mut matches = Vec::new();
		for _ in 0..self.rules.len() {
			matches.push((0usize, None));
		}

		let content = source.content();
		let mut cursor = Cursor::new(0usize, doc.source()); // Cursor in file

		if let Some(parent) = parent
		// Terminate parent's paragraph state
		{
			self.handle_reports(
				parent.source(),
				self.state_mut()
					.on_scope_end(self, parent, super::state::Scope::PARAGRAPH),
			);
		}

		loop {
			let (rule_pos, rule, match_data) = self.update_matches(&cursor, &mut matches);

			// Unmatched content
			let text_content =
				util::process_text(&doc, &content.as_str()[cursor.pos..rule_pos.pos]);
			if !text_content.is_empty() {
				self.push(
					&doc,
					Box::new(Text::new(
						Token::new(cursor.pos..rule_pos.pos, source.clone()),
						text_content,
					)),
				);
			}

			if let Some(rule) = rule {
				// Rule callback
				let dd: &'a dyn Document = unsafe { std::mem::transmute(&doc as &dyn Document) };
				let (new_cursor, reports) = rule.on_match(self, dd, rule_pos, match_data);

				self.handle_reports(doc.source(), reports);

				// Advance
				cursor = new_cursor;
			} else
			// No rules left
			{
				break;
			}
		}

		// State
		self.handle_reports(
			doc.source(),
			self.state_mut()
				.on_scope_end(self, &doc, super::state::Scope::DOCUMENT),
		);

		self.push(
			&doc,
			Box::new(DocumentEnd(Token::new(
				doc.source().content().len()..doc.source().content().len(),
				doc.source(),
			))),
		);

		return Box::new(doc);
	}

	fn parse_into<'a>(&self, source: Rc<dyn Source>, document: &'a dyn Document<'a>) {
		let mut matches = Vec::new();
		for _ in 0..self.rules.len() {
			matches.push((0usize, None));
		}

		let content = source.content();
		let mut cursor = Cursor::new(0usize, source.clone());

		loop {
			let (rule_pos, rule, match_data) = self.update_matches(&cursor, &mut matches);

			// Unmatched content
			let text_content =
				util::process_text(document, &content.as_str()[cursor.pos..rule_pos.pos]);
			if !text_content.is_empty() {
				self.push(
					document,
					Box::new(Text::new(
						Token::new(cursor.pos..rule_pos.pos, source.clone()),
						text_content,
					)),
				);
			}

			if let Some(rule) = rule {
				// Rule callback
				let (new_cursor, reports) = (*rule).on_match(self, document, rule_pos, match_data);

				self.handle_reports(document.source(), reports);

				// Advance
				cursor = new_cursor;
			} else
			// No rules left
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

impl KernelHolder for LangParser {
	fn get_kernel(&self, name: &str) -> Option<RefMut<'_, Kernel>> {
		RefMut::filter_map(self.kernels.borrow_mut(), |map| map.get_mut(name)).ok()
	}

	fn insert_kernel(&self, name: String, kernel: Kernel) -> RefMut<'_, Kernel> {
		//TODO do not get
		self.kernels.borrow_mut().insert(name.clone(), kernel);
		self.get_kernel(name.as_str()).unwrap()
	}
}

impl StyleHolder for LangParser {
	fn element_styles(&self) -> Ref<'_, HashMap<String, Rc<dyn ElementStyle>>> {
		self.styles.borrow()
	}

	fn element_styles_mut(&self) -> RefMut<'_, HashMap<String, Rc<dyn ElementStyle>>> {
		self.styles.borrow_mut()
	}
}

impl LayoutHolder for LangParser {
	fn layouts(&self) -> Ref<'_, HashMap<String, Rc<dyn LayoutType>>> { self.layouts.borrow() }

	fn layouts_mut(&self) -> RefMut<'_, HashMap<String, Rc<dyn LayoutType>>> {
		self.layouts.borrow_mut()
	}
}

impl CustomStyleHolder for LangParser {
	fn custom_styles(&self) -> Ref<'_, HashMap<String, Rc<dyn CustomStyle>>> {
		self.custom_styles.borrow()
	}

	fn custom_styles_mut(&self) -> RefMut<'_, HashMap<String, Rc<dyn CustomStyle>>> {
		self.custom_styles.borrow_mut()
	}
}
