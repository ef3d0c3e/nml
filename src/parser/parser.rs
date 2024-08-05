use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;
use ariadne::Report;
use unicode_segmentation::UnicodeSegmentation;

use super::customstyle::CustomStyleHolder;
use super::layout::LayoutHolder;
use super::rule::Rule;
use super::source::Cursor;
use super::source::Source;
use super::state::RuleStateHolder;
use super::style::StyleHolder;
use crate::document::document::Document;
use crate::document::document::DocumentAccessors;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::elements::customstyle::CustomStyleRule;
use crate::elements::paragraph::Paragraph;
use crate::lua::kernel::Kernel;
use crate::lua::kernel::KernelHolder;
use crate::parser::source::SourceFile;
use ariadne::Color;

#[derive(Debug)]
pub struct ReportColors {
	pub error: Color,
	pub warning: Color,
	pub info: Color,
	pub highlight: Color,
}

impl ReportColors {
	pub fn with_colors() -> Self {
		Self {
			error: Color::Red,
			warning: Color::Yellow,
			info: Color::BrightBlue,
			highlight: Color::BrightMagenta,
		}
	}

	pub fn without_colors() -> Self {
		Self {
			error: Color::Primary,
			warning: Color::Primary,
			info: Color::Primary,
			highlight: Color::Primary,
		}
	}
}

/// The state that is shared with the state's children
pub struct SharedState {
	pub rule_state: RuleStateHolder,

	/// The lua [`Kernel`]s
	pub kernels: KernelHolder,

	/// The styles
	pub styles: StyleHolder,

	/// The layouts
	pub layouts: LayoutHolder,

	/// The custom styles
	pub custom_styles: CustomStyleHolder,
}

impl SharedState {
	/// Construct a new empty shared state
	pub(self) fn new(parser: &dyn Parser) -> Self {
		let mut s = Self {
			rule_state: RuleStateHolder::default(),
			kernels: KernelHolder::default(),
			styles: StyleHolder::default(),
			layouts: LayoutHolder::default(),
			custom_styles: CustomStyleHolder::default(),
		};

		// Register default kernel
		s.kernels
			.insert("main".to_string(), Kernel::new(parser));

		parser.rules().iter().for_each(|rule| {
			rule.register_styles(&mut s.styles);
			rule.register_layouts(&mut s.layouts);
		});

		s
	}
}

/// The state of the parser
pub struct ParserState<'a, 'b> {
	/// The parser for which this state exists
	pub parser: &'a dyn Parser,

	/// The (optional) parent state
	parent: Option<&'b ParserState<'a, 'b>>,

	/// The position of the matches in the current state
	matches: RefCell<Vec<(usize, Option<Box<dyn Any>>)>>,

	/// State shared among all states
	pub shared: Rc<RefCell<SharedState>>,
}

impl<'a, 'b> ParserState<'a, 'b> {
	/// Constructs a new state for a given parser with an optional parent
	///
	/// Parent should be None when parsing a brand new document.
	/// If you have to set the parent to Some(..) (e.g for imports or sub-document),
	/// be sure to use the [`ParserState::with_state`] method instead, this create a
	/// RAII lived state for use within bounded lifetime.
	pub fn new(parser: &'a dyn Parser, parent: Option<&'a ParserState<'a, 'b>>) -> Self {
		let matches = parser.rules().iter().map(|_| (0, None)).collect::<Vec<_>>();
		let shared = if let Some(parent) = &parent {
			parent.shared.clone()
		} else {
			Rc::new(RefCell::new(SharedState::new(parser)))
		};

		Self {
			parser,
			parent,
			matches: RefCell::new(matches),
			shared,
		}
	}

	/// Adds a new rule to the current state
	///
	/// This method will recursively modify the parent states's matches
	///
	/// # Errors
	///
	/// Will fail if:
	///  * The name for the new rule clashes with an already existing rule
	///  * If after is Some(..), not finding the rule to insert after
	/// On failure, it is safe to continue using this state, however the added rule won't exists.
	/*
	pub fn add_rule(
		&mut self,
		rule: Box<dyn Rule>,
		after: Option<&'static str>,
	) -> Result<(), String> {
		// FIXME: This method should not modify the parser
		// Instead we should have some sort of list of references to rules
		// Also need to add a sorting key for rules, so they can be automatically registered, then sorted
	
		// TODO2: Should also check for duplicate rules name when creating bindings...
		// Error on duplicate rule
		if let Some(_) = self
			.parser
			.rules()
			.iter()
			.find(|other_rule| other_rule.name() == rule.name())
		{
			return Err(format!(
				"Attempted to introduce duplicate rule: `{}`",
				rule.name()
			));
		}

		// Try to insert after
		if let Some(after) = after {
			let index =
				self.parser.rules()
					.iter()
					.enumerate()
					.find(|(_, rule)| rule.name() == after)
					.map(|(idx, _)| idx);

			if let Some(index) = index {
				self.parser.rules_mut().insert(index, rule);
			} else {
				return Err(format!("Unable to find rule `{after}` to insert after"));
			}
		} else {
			self.parser.rules_mut().push(rule);
		}

		// Carry out the `matches` modification
		fn carry(state: &ParserState) {
			state.matches.borrow_mut().push((0, None));

			if let Some(parent) = state.parent {
				carry(parent);
			}
		}
		carry(self);

		// TODO2: Carry on bindings, style, layouts registration... into self.shared
		Ok(())
	}
	*/

	/// Runs a procedure with a new state that inherits it's [`SharedState`] state from self
	///
	/// Note: When parsing a new document, create a default state, then the parsing process
	/// creates states using this method
	pub fn with_state<F, R>(&self, f: F) -> R
	where
		F: FnOnce(ParserState) -> R,
	{
		let new_state = ParserState::new(self.parser, Some(self));
		f(new_state)
	}

	fn handle_reports(
		&self,
		source: Rc<dyn Source>,
		reports: Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>,
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

	/// Updates matches from a given start position e.g [`Cursor`]
	///
	/// # Return
	///  1. The cursor position after updating the matches
	///  2. (Optional) The winning match with it's match data
	///
	/// If the winning match is None, it means that the document has no more rule to match
	/// I.E The rest of the content should be added as a [`Text`] element.
	/// The match data should be passed to the [`Rule::on_match`] method
	pub fn update_matches(
		&self,
		cursor: &Cursor,
	) -> (Cursor, Option<(usize, Box<dyn Any>)>) {
		let mut matches_borrow = self.matches.borrow_mut();

		self.parser.rules()
			.iter()
			.zip(matches_borrow.iter_mut())
			.for_each(|(rule, (matched_at, match_data))| {
				// Don't upate if not stepped over yet
				if *matched_at > cursor.pos && rule.downcast_ref::<CustomStyleRule>().is_none() {
					// TODO: maybe we should expose matches() so it becomes possible to dynamically register a new rule
					return;
				}

				(*matched_at, *match_data) = match rule.next_match(self, cursor) {
					None => (usize::MAX, None),
					Some((mut pos, mut data)) => {
						// Check if escaped
						while pos != usize::MAX {
							let content = cursor.source.content().as_str();
							let mut graphemes = content[0..pos].graphemes(true);
							let mut escaped = false;
							'inner: loop {
								let g = graphemes.next_back();
								if !g.is_some() || g.unwrap() != "\\" {
									break 'inner;
								}

								escaped = !escaped;
							}
							if !escaped {
								break;
							}

							// Find next potential match
							(pos, data) = match rule.next_match(self, &cursor.at(pos + 1)) {
								Some((new_pos, new_data)) => (new_pos, new_data),
								None => (usize::MAX, data), // Stop iterating
							}
						}

						(pos, (pos != usize::MAX).then_some(data))
					}
				}
			});

		// Get winning match
		let (winner, next_pos) = matches_borrow
			.iter()
			.enumerate()
			.min_by_key(|(_, (pos, _))| pos)
			.map(|(winner, (pos, _))| (winner, *pos))
			.unwrap();

		if next_pos == usize::MAX // No rule has matched
		{
			let content = cursor.source.content();
			// No winners, i.e no matches left
			return (cursor.at(content.len()), None);
		}

		return (cursor.at(next_pos),
			Some((winner, matches_borrow[0].1.take().unwrap())))
		
	}

	/// Add an [`Element`] to the [`Document`]
	fn push(&mut self, doc: &dyn Document, elem: Box<dyn Element>) {
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
					self.shared.rule_state
						.on_scope_end(&mut self, doc, super::state::Scope::PARAGRAPH),
				);
			}

			doc.push(elem);
		}
	}
}

pub trait Parser {
	/// Gets the colors for formatting errors
	///
	/// When colors are disabled, all colors should resolve to empty string
	fn colors(&self) -> &ReportColors;

	/// Gets a reference to all the [`Rule`]s defined for the parser
	fn rules(&self) -> &Vec<Box<dyn Rule>>;
	/// Gets a mutable reference to all the [`Rule`]s defined for the parser
	fn rules_mut(&mut self) -> &mut Vec<Box<dyn Rule>>;

	/// Whether the parser emitted an error during it's parsing process
	fn has_error(&self) -> bool;

	/// Add an [`Element`] to the [`Document`]
	fn push<'a>(&self, doc: &dyn Document, elem: Box<dyn Element>);
	
	/// Parse [`Source`] into a new [`Document`]
	///
	/// # Errors
	///
	/// This method will not fail because we try to optimistically recover from parsing errors.
	/// However the resulting document should not get compiled if an error has happened
	/// see [`Parser::has_error()`] for reference
	fn parse<'a>(
		&self,
		state: ParserState,
		source: Rc<dyn Source>,
		parent: Option<&'a dyn Document<'a>>,
	) -> Box<dyn Document<'a> + 'a>;

	/// Parse [`Source`] into an already existing [`Document`]
	///
	/// # Errors
	///
	/// This method will not fail because we try to optimistically recover from parsing errors.
	/// However the resulting document should not get compiled if an error has happened
	/// see [`Parser::has_error()`] for reference
	fn parse_into<'a>(&self,
		state: ParserState,
		source: Rc<dyn Source>, document: &'a dyn Document<'a>);

	fn add_rule(
		&mut self,
		rule: Box<dyn Rule>,
		after: Option<&'static str>,
	) -> Result<(), String> {
		if let Some(_) = self
			.rules()
			.iter()
			.find(|other_rule| other_rule.name() == rule.name())
		{
			return Err(format!(
				"Attempted to introduce duplicate rule: `{}`",
				rule.name()
			));
		}

		// Try to insert after
		if let Some(after) = after {
			let index =
				self.rules()
					.iter()
					.enumerate()
					.find(|(_, rule)| rule.name() == after)
					.map(|(idx, _)| idx);

			if let Some(index) = index {
				self.rules_mut().insert(index, rule);
			} else {
				return Err(format!("Unable to find rule `{after}` to insert after"));
			}
		} else {
			self.rules_mut().push(rule);
		}

		Ok(())
	}
}
