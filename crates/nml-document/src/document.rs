use std::fmt;
use std::sync::Arc;

use nml_source::Source;
use nml_source::Token;

use crate::Attributes;
use crate::Element;
use crate::ElementId;
use crate::ElementKind;
use crate::Slot;
use crate::SlotId;

/// The element tree of a unit. It is only built through a [`DocumentBuilder`] and cannot be
/// changed afterwards.
#[derive(Debug, Clone)]
pub struct Document {
	source: Arc<Source>,
	elements: Vec<Element>,
	slots: Vec<Slot>,
	root: ElementId,
}

impl Document {
	pub fn source(&self) -> &Arc<Source> {
		&self.source
	}

	pub fn root(&self) -> ElementId {
		self.root
	}

	pub fn get(&self, id: ElementId) -> &Element {
		&self.elements[id.0 as usize]
	}

	pub fn slot(&self, id: SlotId) -> &Slot {
		&self.slots[id.0 as usize]
	}

	/// Every slot, in the order they were added.
	pub fn slots(&self) -> impl Iterator<Item = (SlotId, &Slot)> {
		self.slots
			.iter()
			.enumerate()
			.map(|(index, slot)| (SlotId(index as u32), slot))
	}

	pub fn children(&self, id: ElementId) -> impl Iterator<Item = (ElementId, &Element)> {
		self.get(id)
			.children
			.iter()
			.map(|child| (*child, self.get(*child)))
	}

	/// The element and everything below it, in document order.
	pub fn descendants(&self, id: ElementId) -> impl Iterator<Item = (ElementId, &Element)> {
		let mut stack = vec![id];
		std::iter::from_fn(move || {
			let id = stack.pop()?;
			let element = self.get(id);
			stack.extend(element.children.iter().rev());
			Some((id, element))
		})
	}

	/// The text below an element, without markup.
	pub fn text(&self, id: ElementId) -> String {
		let mut text = String::new();
		collect_text(&self.elements, &self.get(id).children, &mut text);
		text
	}
}

fn collect_text(elements: &[Element], ids: &[ElementId], out: &mut String) {
	for id in ids {
		let element = &elements[id.0 as usize];
		match &element.kind {
			ElementKind::Text(text) => out.push_str(text),
			_ => collect_text(elements, &element.children, out),
		}
	}
}

/// An element about to be written, as element listeners see it. Only its attributes can
/// change.
#[derive(Debug)]
pub struct Draft<'a> {
	pub kind: &'a ElementKind,
	pub token: &'a Token,
	pub attributes: &'a mut Attributes,
	children: &'a [ElementId],
	written: &'a [Element],
}

impl Draft<'_> {
	/// The element's children, which are already written.
	pub fn children(&self) -> impl Iterator<Item = &Element> {
		self.children
			.iter()
			.map(|child| &self.written[child.0 as usize])
	}

	/// The text below the element, without markup.
	pub fn text(&self) -> String {
		let mut text = String::new();
		collect_text(self.written, self.children, &mut text);
		text
	}
}

/// Called with every element before it is written.
pub type Listener<'l> = Box<dyn for<'a> FnMut(Draft<'a>) + 'l>;

/// An element whose children are still being built.
#[derive(Debug)]
struct Open {
	kind: ElementKind,
	token: Token,
	attributes: Attributes,
	children: Vec<ElementId>,
}

/// Builds a [`Document`]. Elements are opened, receive their children, and are written when
/// closed: children are written before their parent.
pub struct DocumentBuilder<'l> {
	source: Arc<Source>,
	elements: Vec<Element>,
	slots: Vec<Slot>,
	/// The open elements; the document root is at the bottom.
	open: Vec<Open>,
	listeners: Vec<Listener<'l>>,
}

impl fmt::Debug for DocumentBuilder<'_> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("DocumentBuilder")
			.field("source", &self.source)
			.field("elements", &self.elements)
			.field("slots", &self.slots)
			.field("open", &self.open)
			.field("listeners", &self.listeners.len())
			.finish()
	}
}

impl<'l> DocumentBuilder<'l> {
	pub fn new(source: Arc<Source>) -> Self {
		let token = Token::new(source.clone(), 0..source.content().len());
		Self {
			source,
			elements: Vec::new(),
			slots: Vec::new(),
			open: vec![Open {
				kind: ElementKind::Document,
				token,
				attributes: Attributes::default(),
				children: Vec::new(),
			}],
			listeners: Vec::new(),
		}
	}

	/// Adds a listener, called with every element before it is written.
	pub fn on_write(&mut self, listener: impl for<'a> FnMut(Draft<'a>) + 'l) {
		self.listeners.push(Box::new(listener));
	}

	/// Opens an element: the elements added until it is closed are its children.
	pub fn open(&mut self, kind: ElementKind, token: Token) {
		self.open.push(Open {
			kind,
			token,
			attributes: Attributes::default(),
			children: Vec::new(),
		});
	}

	/// Opens an element with attributes.
	pub fn open_with(&mut self, kind: ElementKind, token: Token, attributes: Attributes) {
		self.open(kind, token);
		self.open.last_mut().unwrap().attributes = attributes;
	}

	/// Closes the innermost open element and writes it.
	///
	/// # Panics
	///
	/// If only the document root is open.
	pub fn close(&mut self) -> ElementId {
		assert!(self.open.len() > 1, "no element to close");
		let open = self.open.pop().unwrap();
		let id = self.write(open);
		self.open.last_mut().unwrap().children.push(id);
		id
	}

	/// Adds an element without children.
	pub fn leaf(&mut self, kind: ElementKind, token: Token) -> ElementId {
		self.open(kind, token);
		self.close()
	}

	/// Adds a slot, and the element that marks where its value goes.
	pub fn slot(&mut self, slot: Slot, token: Token) -> SlotId {
		let id = self.add_slot(slot);
		self.leaf(ElementKind::Slot(id), token);
		id
	}

	/// Adds a slot that an element refers to, such as a reference's URL.
	pub fn add_slot(&mut self, slot: Slot) -> SlotId {
		let id = SlotId(u32::try_from(self.slots.len()).expect("too many slots"));
		self.slots.push(slot);
		id
	}

	/// Fills a slot before the document is finished: a post-parse slot gets its value.
	///
	/// # Panics
	///
	/// If the slot does not exist.
	pub fn fill_slot(&mut self, id: SlotId, slot: Slot) {
		self.slots[id.0 as usize] = slot;
	}

	/// The number of open elements, the document root included.
	pub fn depth(&self) -> usize {
		self.open.len()
	}

	/// Writes the root and returns the document.
	///
	/// # Panics
	///
	/// If an element is still open.
	pub fn finish(mut self) -> Document {
		assert!(self.open.len() == 1, "elements are still open");
		let root = self.open.pop().unwrap();
		let root = self.write(root);
		Document {
			source: self.source,
			elements: self.elements,
			slots: self.slots,
			root,
		}
	}

	fn write(&mut self, mut open: Open) -> ElementId {
		for listener in &mut self.listeners {
			listener(Draft {
				kind: &open.kind,
				token: &open.token,
				attributes: &mut open.attributes,
				children: &open.children,
				written: &self.elements,
			});
		}
		let id = ElementId(u32::try_from(self.elements.len()).expect("too many elements"));
		for child in &open.children {
			self.elements[child.0 as usize].parent = Some(id);
		}
		self.elements.push(Element {
			kind: open.kind,
			token: open.token,
			attributes: open.attributes,
			parent: None,
			children: open.children,
		});
		id
	}
}

#[cfg(test)]
mod tests {
	use std::cell::RefCell;

	use nml_source::Url;

	use super::*;
	use crate::Style;

	fn source() -> Arc<Source> {
		Arc::new(Source::new(
			Url::parse("file:///a.nml").unwrap(),
			"# Title\n\nsome **bold** text".into(),
			0,
		))
	}

	fn token(source: &Arc<Source>, range: std::ops::Range<usize>) -> Token {
		Token::new(source.clone(), range)
	}

	fn build(builder: &mut DocumentBuilder, source: &Arc<Source>) {
		builder.open(ElementKind::Heading { level: 1 }, token(source, 0..7));
		builder.leaf(ElementKind::Text("Title".into()), token(source, 2..7));
		builder.close();
		builder.open(ElementKind::Paragraph, token(source, 9..27));
		builder.leaf(ElementKind::Text("some ".into()), token(source, 9..14));
		builder.open(ElementKind::Style(Style::Bold), token(source, 14..22));
		builder.leaf(ElementKind::Text("bold".into()), token(source, 16..20));
		builder.close();
		builder.leaf(ElementKind::Text(" text".into()), token(source, 22..27));
		builder.close();
	}

	#[test]
	fn tree() {
		let source = source();
		let mut builder = DocumentBuilder::new(source.clone());
		build(&mut builder, &source);
		let document = builder.finish();

		let root = document.root();
		assert_eq!(document.get(root).kind, ElementKind::Document);
		let kinds: Vec<_> = document
			.descendants(root)
			.map(|(_, element)| element.kind.name())
			.collect();
		assert_eq!(
			kinds,
			[
				"document",
				"heading",
				"text",
				"paragraph",
				"text",
				"style",
				"text",
				"text"
			]
		);
		let (paragraph, _) = document.children(root).nth(1).unwrap();
		assert_eq!(document.text(paragraph), "some bold text");
		assert_eq!(document.get(paragraph).parent(), Some(root));
		let (bold, _) = document.children(paragraph).nth(1).unwrap();
		assert_eq!(document.get(bold).parent(), Some(paragraph));
	}

	#[test]
	fn listeners_change_elements_before_they_are_written() {
		let source = source();
		let seen = RefCell::new(Vec::new());
		let mut builder = DocumentBuilder::new(source.clone());
		builder.on_write(|draft| {
			if let ElementKind::Heading { level } = draft.kind {
				let title = draft.text();
				seen.borrow_mut().push((*level, title.clone()));
				draft.attributes.id = Some(title.to_lowercase());
				draft.attributes.classes.push("numbered".into());
			}
		});
		build(&mut builder, &source);
		let document = builder.finish();

		assert_eq!(*seen.borrow(), [(1, "Title".to_string())]);
		let (heading, element) = document.children(document.root()).next().unwrap();
		assert_eq!(element.attributes.id.as_deref(), Some("title"));
		assert_eq!(element.attributes.classes, ["numbered"]);
		assert_eq!(document.text(heading), "Title");
	}

	#[test]
	#[should_panic(expected = "elements are still open")]
	fn unclosed_elements() {
		let source = source();
		let mut builder = DocumentBuilder::new(source.clone());
		builder.open(ElementKind::Paragraph, token(&source, 0..1));
		builder.finish();
	}

	#[test]
	#[should_panic(expected = "no element to close")]
	fn closing_the_root() {
		DocumentBuilder::new(source()).close();
	}

	#[test]
	fn slots() {
		use crate::LinkSlot;
		use crate::OutputContext;
		use crate::Property;
		use crate::SymbolRef;

		let source = source();
		let mut builder = DocumentBuilder::new(source.clone());
		builder.open(ElementKind::Paragraph, token(&source, 9..27));
		let link = Slot::Link(LinkSlot {
			target: SymbolRef::local("title"),
			property: Property::Title,
			context: OutputContext::Text,
		});
		let id = builder.slot(link.clone(), token(&source, 14..22));
		builder.close();
		let document = builder.finish();

		assert_eq!(document.slot(id), &link);
		assert_eq!(document.slots().count(), 1);
		let (paragraph, _) = document.children(document.root()).next().unwrap();
		let (_, element) = document.children(paragraph).next().unwrap();
		assert_eq!(element.kind, ElementKind::Slot(id));
	}
}
