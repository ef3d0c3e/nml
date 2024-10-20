use ariadne::Label;
use ariadne::Report;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;
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
use crate::elements::paragraph::Paragraph;
use crate::lsp::semantic::SemanticsHolder;
use crate::lua::kernel::Kernel;
use crate::lua::kernel::KernelHolder;
use crate::parser::source::SourceFile;
use crate::parser::source::VirtualSource;
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

/// The state that is shared with the state's childre
pub struct SharedState {
	pub rule_state: RefCell<RuleStateHolder>,

	/// The lua [`Kernel`]s
	pub kernels: RefCell<KernelHolder>,

	/// The styles
	pub styles: RefCell<StyleHolder>,

	/// The layouts
	pub layouts: RefCell<LayoutHolder>,

	/// The custom styles
	pub custom_styles: RefCell<CustomStyleHolder>,

	/// The semantics
	pub semantics: Option<RefCell<SemanticsHolder>>,
}

impl SharedState {
	/// Construct a new empty shared state
	pub(self) fn new(parser: &dyn Parser, enable_semantics: bool) -> Self {
		let s = Self {
			rule_state: RefCell::new(RuleStateHolder::default()),
			kernels: RefCell::new(KernelHolder::default()),
			styles: RefCell::new(StyleHolder::default()),
			layouts: RefCell::new(LayoutHolder::default()),
			custom_styles: RefCell::new(CustomStyleHolder::default()),
			semantics: enable_semantics.then_some(RefCell::new(SemanticsHolder::new())),
		};

		// Register default kernel
		s.kernels
			.borrow_mut()
			.insert("main".to_string(), Kernel::new(parser));

		// Default styles & layouts
		parser.rules().iter().for_each(|rule| {
			rule.register_styles(&mut s.styles.borrow_mut());
			rule.register_layouts(&mut s.layouts.borrow_mut());
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
	pub shared: Rc<SharedState>,
}

/// Represents the state of the parser
///
/// This state has some shared data from [`SharedState`] which gets shared
/// with the children of that state, see [`ParserState::with_state`]
impl<'a, 'b> ParserState<'a, 'b> {
	/// Constructs a new state for a given parser with an optional parent
	///
	/// Parent should be None when parsing a brand new document. If you have to
	/// set the parent to Some(..) (e.g for imports or sub-document), be sure
	/// to use the [`ParserState::with_state`] method instead, this create a
	/// RAII lived state for use within bounded lifetime.
	pub fn new(parser: &'a dyn Parser, parent: Option<&'a ParserState<'a, 'b>>) -> Self {
		let matches = parser.rules().iter().map(|_| (0, None)).collect::<Vec<_>>();
		let shared = if let Some(parent) = &parent {
			parent.shared.clone()
		} else {
			Rc::new(SharedState::new(parser, false))
		};

		Self {
			parser,
			parent,
			matches: RefCell::new(matches),
			shared,
		}
	}

	/// Constructs a new state with semantics enabled
	/// See [`ParserState::new`] for mote information
	pub fn new_with_semantics(
		parser: &'a dyn Parser,
		parent: Option<&'a ParserState<'a, 'b>>,
	) -> Self {
		let matches = parser.rules().iter().map(|_| (0, None)).collect::<Vec<_>>();
		let shared = if let Some(parent) = &parent {
			parent.shared.clone()
		} else {
			Rc::new(SharedState::new(parser, true))
		};

		Self {
			parser,
			parent,
			matches: RefCell::new(matches),
			shared,
		}
	}

	/// Runs a procedure with a new state that inherits the [`SharedState`] state from [`self`]
	///
	/// Note: When parsing a new document, create a new state, then the parsing process
	/// creates states using this method
	pub fn with_state<F, R>(&self, f: F) -> R
	where
		F: FnOnce(ParserState) -> R,
	{
		let new_state = ParserState::new(self.parser, Some(self));
		f(new_state)
	}

	/// Updates matches from a given start position e.g [`Cursor`]
	///
	/// # Return
	///
	///  1. The cursor position after updating the matches
	///  2. (Optional) The winning match with it's match data
	/// If the winning match is None, it means that the document has no more
	/// rule to match. I.e The rest of the content should be added as a
	/// [`Text`] element.
	/// The match data should be passed to the [`Rule::on_match`] method.
	///
	/// # Strategy
	///
	/// This function call [`Rule::next_match`] on the rules defined for the
	/// parser. It then takes the rule that has the closest `next_match` and
	/// returns it. If next_match starts on an escaped character i.e `\\`,
	/// then it starts over to find another match for that rule.
	/// In case multiple rules have the same `next_match`, the rules that are
	/// defined first in the parser are prioritized. See [Parser::add_rule] for
	/// information on how to prioritize rules.
	///
	/// Notes that the result of every call to [`Rule::next_match`] gets stored
	/// in a table: [`ParserState::matches`]. Until the cursor steps over a
	/// position in the table, `next_match` won't be called.
	pub fn update_matches(
		&self,
		mode: &ParseMode,
		cursor: &Cursor,
	) -> (Cursor, Option<(usize, Box<dyn Any>)>) {
		let mut matches_borrow = self.matches.borrow_mut();

		self.parser
			.rules()
			.iter()
			.zip(matches_borrow.iter_mut())
			.for_each(|(rule, (matched_at, match_data))| {
				// Don't upate if not stepped over yet
				if *matched_at > cursor.pos {
					return;
				}

				(*matched_at, *match_data) = match rule.next_match(&mode, self, cursor) {
					None => (usize::MAX, None),
					Some((mut pos, mut data)) => {
						// Check if escaped
						while pos != usize::MAX {
							let content = cursor.source.content().as_str();
							let mut graphemes = content[0..pos].graphemes(true);
							let mut escaped = false;
							'inner: loop {
								let g = graphemes.next_back();
								if g.is_none() || g.unwrap() != "\\" {
									break 'inner;
								}

								escaped = !escaped;
							}
							if !escaped {
								break;
							}

							// Find next potential match
							(pos, data) = match rule.next_match(&mode, self, &cursor.at(pos + 1)) {
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

		if next_pos == usize::MAX
		// No rule has matched
		{
			let content = cursor.source.content();
			// No winners, i.e no matches left
			return (cursor.at(content.len()), None);
		}

		(
			cursor.at(next_pos),
			Some((winner, matches_borrow[winner].1.take().unwrap())),
		)
	}

	/// Add an [`Element`] to the [`Document`]
	pub fn push(&self, doc: &dyn Document, elem: Box<dyn Element>) {
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
				self.parser
					.handle_reports(self.shared.rule_state.borrow_mut().on_scope_end(
						self,
						doc,
						super::state::Scope::PARAGRAPH,
					));
			}

			doc.push(elem);
		}
	}

	/// Resets the position and the match_data for a given rule. This is used
	/// in order to have 'dynamic' rules that may not match at first, but their
	/// matching rule is modified through the parsing process.
	///
	/// This function also recursively calls itself on it's `parent`, in order
	/// to fully reset the match.
	///
	/// See [`CustomStyleRule`] for an example of how this is used.
	///
	/// # Error
	///
	/// Returns an error if `rule_name` was not found in the parser's ruleset.
	pub fn reset_match(&self, rule_name: &str) -> Result<(), String> {
		if self
			.parser
			.rules()
			.iter()
			.zip(self.matches.borrow_mut().iter_mut())
			.try_for_each(|(rule, (match_pos, match_data))| {
				if rule.name() != rule_name {
					return Ok(());
				}

				*match_pos = 0;
				match_data.take();
				Err(())
			})
			.is_ok()
		{
			return Err(format!("Could not find rule: {rule_name}"));
		}

		// Resurcively reset
		if let Some(parent) = self.parent {
			return parent.reset_match(rule_name);
		}

		Ok(())
	}
}

pub struct ParseMode {
	pub paragraph_only: bool,
}

impl Default for ParseMode {
	fn default() -> Self {
		Self {
			paragraph_only: false,
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

	/// Parse [`Source`] into a new [`Document`]
	///
	/// # Errors
	///
	/// This method will not fail because we try to optimistically recover from
	/// parsing errors. However the resulting document should not get compiled
	/// if an error has happenedn, see [`Parser::has_error()`] for reference
	///
	/// # Returns
	///
	/// This method returns the resulting [`Document`] after psrsing `source`,
	/// note that the [`ParserState`] is only meant to perform testing and not
	/// meant to be reused.
	fn parse<'p, 'a, 'doc>(
		&'p self,
		state: ParserState<'p, 'a>,
		source: Rc<dyn Source>,
		parent: Option<&'doc dyn Document<'doc>>,
		mode: ParseMode,
	) -> (Box<dyn Document<'doc> + 'doc>, ParserState<'p, 'a>);

	/// Parse [`Source`] into an already existing [`Document`]
	///
	/// # Errors
	///
	/// This method will not fail because we try to optimistically recover from
	/// parsing errors. However the resulting document should not get compiled
	/// if an error has happened see [`Parser::has_error()`] for reference
	///
	/// # Returns
	///
	/// The returned [`ParserState`] is not meant to be reused, it's meant for
	/// testing.
	fn parse_into<'p, 'a, 'doc>(
		&'p self,
		state: ParserState<'p, 'a>,
		source: Rc<dyn Source>,
		document: &'doc dyn Document<'doc>,
		mode: ParseMode,
	) -> ParserState<'p, 'a>;

	/// Adds a rule to the parser.
	///
	/// # Warning
	///
	/// This method must not be called if a [`ParserState`] for this parser exists.
	fn add_rule(&mut self, rule: Box<dyn Rule>) -> Result<(), String> {
		if self
			.rules()
			.iter()
			.any(|other_rule| other_rule.name() == rule.name())
		{
			return Err(format!(
				"Attempted to introduce duplicate rule: `{}`",
				rule.name()
			));
		}

		self.rules_mut().push(rule);

		Ok(())
	}

	/// Handles the reports produced by parsing. The default is to output them
	/// to stderr, but you are free to modify it.
	fn handle_reports(&self, reports: Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>>) {
		for mut report in reports {
			let mut sources: HashSet<Rc<dyn Source>> = HashSet::new();
			fn recurse_source(sources: &mut HashSet<Rc<dyn Source>>, source: Rc<dyn Source>) {
				sources.insert(source.clone());
				if let Some(parent) = source.location() {
					let parent_source = parent.source();
					if sources.get(&parent_source).is_none() {
						recurse_source(sources, parent_source);
					}
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
							+ if location.source().content().as_bytes()[location.start()] == b'\n' {
								1
							} else {
								0
							};
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
