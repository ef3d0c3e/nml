use nml_document::Checkbox;
use nml_document::Document;
use nml_document::ElementId;
use nml_document::ElementKind;
use nml_document::ListKind;
use nml_document::OutputContext;
use nml_document::Slot;
use nml_document::SlotId;
use nml_document::Style;

/// A compiled page with nothing at its slots. It is never modified: filling the slots
/// streams it, so the positions stay valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
	pub content: String,
	/// In order of position.
	pub slots: Vec<TemplateSlot>,
}

/// Where a slot's value goes in a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateSlot {
	pub slot: SlotId,
	/// A byte offset in the template.
	pub position: usize,
	pub context: OutputContext,
}

/// The stylesheet of highlighted code, in the output directory. Pages with code blocks link
/// to it.
pub const CODE_STYLESHEET: &str = "nml-code.css";

/// The page around a document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageOptions {
	/// The `lang` of the page.
	pub language: Option<String>,
	pub title: String,
	/// Stylesheet URLs.
	pub stylesheets: Vec<String>,
	pub icon: Option<String>,
}

/// Compiles a document into a full HTML page.
pub fn compile(document: &Document, options: &PageOptions) -> Template {
	let mut out = Output {
		content: String::new(),
		slots: Vec::new(),
	};
	out.raw("<!DOCTYPE html>\n<html");
	if let Some(language) = &options.language {
		out.raw(" lang=\"");
		out.attribute(language);
		out.raw("\"");
	}
	out.raw(">\n<head>\n<meta charset=\"utf-8\">\n");
	out.raw("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
	out.raw("<title>");
	out.text(&options.title);
	out.raw("</title>\n");
	for stylesheet in &options.stylesheets {
		out.raw("<link rel=\"stylesheet\" href=\"");
		out.attribute(stylesheet);
		out.raw("\">\n");
	}
	if let Some(icon) = &options.icon {
		out.raw("<link rel=\"icon\" href=\"");
		out.attribute(icon);
		out.raw("\">\n");
	}
	out.raw("</head>\n<body>\n");
	out.element(document, document.root());
	out.raw("</body>\n</html>\n");
	Template {
		content: out.content,
		slots: out.slots,
	}
}

struct Output {
	content: String,
	slots: Vec<TemplateSlot>,
}

impl Output {
	fn raw(&mut self, text: &str) {
		self.content.push_str(text);
	}

	fn text(&mut self, text: &str) {
		escape(&mut self.content, text, false);
	}

	fn attribute(&mut self, text: &str) {
		escape(&mut self.content, text, true);
	}

	fn children(&mut self, document: &Document, id: ElementId) {
		for child in document.get(id).children() {
			self.element(document, *child);
		}
	}

	/// An opening tag with the element's id and classes.
	fn open(&mut self, document: &Document, id: ElementId, tag: &str) {
		self.open_with(document, id, tag, None, "");
	}

	/// An opening tag with the element's id, `class` then the element's classes, and
	/// `extra`: raw attributes that start with a blank.
	fn open_with(
		&mut self,
		document: &Document,
		id: ElementId,
		tag: &str,
		class: Option<&str>,
		extra: &str,
	) {
		let attributes = &document.get(id).attributes;
		self.raw("<");
		self.raw(tag);
		if let Some(id) = &attributes.id {
			self.raw(" id=\"");
			self.attribute(id);
			self.raw("\"");
		}
		let classes: Vec<&str> = class
			.into_iter()
			.chain(attributes.classes.iter().map(String::as_str))
			.collect();
		if !classes.is_empty() {
			self.raw(" class=\"");
			self.attribute(&classes.join(" "));
			self.raw("\"");
		}
		self.raw(extra);
		self.raw(">");
	}

	fn wrap(&mut self, document: &Document, id: ElementId, tag: &str) {
		self.open(document, id, tag);
		self.children(document, id);
		self.raw("</");
		self.raw(tag);
		self.raw(">");
	}

	/// A list item. An item with a single paragraph is tight: the paragraph's content goes
	/// in the item without `<p>`.
	fn list_item(&mut self, document: &Document, id: ElementId, checkbox: Option<Checkbox>) {
		self.open(document, id, "li");
		match checkbox {
			Some(Checkbox::Unchecked) => self.raw("<input type=\"checkbox\" disabled> "),
			Some(Checkbox::Partial) => {
				self.raw("<input type=\"checkbox\" disabled aria-checked=\"mixed\"> ")
			}
			Some(Checkbox::Checked) => self.raw("<input type=\"checkbox\" disabled checked> "),
			None => {}
		}
		let children = document.children(id);
		let paragraphs = document
			.children(id)
			.filter(|(_, child)| child.kind == ElementKind::Paragraph)
			.count();
		for (child, element) in children {
			if paragraphs == 1 && element.kind == ElementKind::Paragraph {
				self.children(document, child);
			} else {
				self.element(document, child);
			}
		}
		self.raw("</li>\n");
	}

	/// A code block: its title, the line numbers in a gutter, and the highlighted code.
	fn code_block(
		&mut self,
		document: &Document,
		id: ElementId,
		language: Option<&str>,
		code: &str,
		first_line: Option<i64>,
	) {
		let mut extra = String::new();
		if let Some(language) = language {
			extra.push_str(" data-language=\"");
			escape(&mut extra, language, true);
			extra.push('"');
		}
		self.open_with(document, id, "figure", Some("code-block"), &extra);
		self.raw("\n");
		if !document.get(id).children().is_empty() {
			self.raw("<figcaption>");
			self.children(document, id);
			self.raw("</figcaption>\n");
		}
		self.raw("<div class=\"code\">");
		if let Some(first) = first_line {
			let lines = code.split('\n').count() as i64;
			self.raw("<pre class=\"line-numbers\" aria-hidden=\"true\">");
			let numbers: Vec<String> = (0..lines)
				.map(|line| first.saturating_add(line).to_string())
				.collect();
			self.raw(&numbers.join("\n"));
			self.raw("</pre>");
		}
		self.raw("<pre><code>");
		self.raw(&nml_render::highlight(code, language));
		self.raw("</code></pre></div>\n</figure>\n");
	}

	/// Records the position of a slot's value.
	fn slot(&mut self, document: &Document, slot: SlotId) {
		let context = match document.slot(slot) {
			Slot::Link(link) => link.context,
			Slot::PostParse(_) | Slot::Deferred(_) => OutputContext::Markup,
			Slot::Text(_) => OutputContext::Text,
		};
		self.slots.push(TemplateSlot {
			slot,
			position: self.content.len(),
			context,
		});
	}

	fn element(&mut self, document: &Document, id: ElementId) {
		match &document.get(id).kind {
			ElementKind::Document => self.children(document, id),
			ElementKind::Paragraph => {
				self.wrap(document, id, "p");
				self.raw("\n");
			}
			ElementKind::Text(text) => self.text(text),
			ElementKind::Style(style) => {
				let tag = match style {
					Style::Bold => "strong",
					Style::Italic => "em",
					Style::Underline => "u",
					Style::Highlight => "mark",
					Style::Strikethrough => "s",
				};
				self.wrap(document, id, tag);
			}
			ElementKind::Heading { level } => {
				self.wrap(document, id, &format!("h{}", level.clamp(&1, &6)));
				self.raw("\n");
			}
			ElementKind::Slot(slot) => match document.slot(*slot) {
				Slot::Text(text) => self.text(text),
				_ => self.slot(document, *slot),
			},
			ElementKind::Reference { href } => {
				self.raw("<a href=\"");
				self.slot(document, *href);
				self.raw("\">");
				self.children(document, id);
				self.raw("</a>");
			}
			ElementKind::Code(code) => {
				self.open(document, id, "code");
				self.text(code);
				self.raw("</code>");
			}
			ElementKind::Link { url } => {
				let mut href = String::from(" href=\"");
				escape(&mut href, url, true);
				href.push('"');
				self.open_with(document, id, "a", None, &href);
				self.children(document, id);
				self.raw("</a>");
			}
			ElementKind::CodeBlock {
				language,
				code,
				first_line,
			} => self.code_block(document, id, language.as_deref(), code, *first_line),
			ElementKind::List(kind) => {
				let tag = match kind {
					ListKind::Bullet => "ul",
					ListKind::Numbered { .. } => "ol",
				};
				let start = match kind {
					ListKind::Numbered { start } if *start != 1 => format!(" start=\"{start}\""),
					_ => String::new(),
				};
				self.open_with(document, id, tag, None, &start);
				self.raw("\n");
				self.children(document, id);
				self.raw("</");
				self.raw(tag);
				self.raw(">\n");
			}
			ElementKind::ListItem { checkbox } => self.list_item(document, id, *checkbox),
			kind => unimplemented!("HTML for `{}`", kind.name()),
		}
	}
}

/// Escapes text for HTML content, or for a double-quoted attribute.
pub fn escape(out: &mut String, text: &str, attribute: bool) {
	for c in text.chars() {
		match c {
			'&' => out.push_str("&amp;"),
			'<' => out.push_str("&lt;"),
			'>' => out.push_str("&gt;"),
			'"' if attribute => out.push_str("&quot;"),
			'\'' if attribute => out.push_str("&#39;"),
			c => out.push(c),
		}
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use nml_document::Attributes;
	use nml_document::DocumentBuilder;
	use nml_document::LinkSlot;
	use nml_document::Property;
	use nml_document::SymbolRef;
	use nml_source::Source;
	use nml_source::Token;
	use nml_source::Url;

	use super::*;

	#[test]
	fn page() {
		let source = Arc::new(Source::new(
			Url::parse("file:///a.nml").unwrap(),
			"x".repeat(10),
			0,
		));
		let token = || Token::new(source.clone(), 0..1);
		let mut builder = DocumentBuilder::new(source.clone());
		let attributes = Attributes {
			id: Some("a\"b".into()),
			classes: vec!["x".into(), "y".into()],
		};
		builder.open_with(ElementKind::Heading { level: 2 }, token(), attributes);
		builder.leaf(ElementKind::Text("1 < 2 & \"3\"".into()), token());
		builder.close();
		builder.open(ElementKind::Paragraph, token());
		builder.leaf(ElementKind::Text("See ".into()), token());
		builder.open(ElementKind::Style(Style::Bold), token());
		let slot = builder.slot(
			Slot::Link(LinkSlot {
				target: SymbolRef::local("a"),
				property: Property::Title,
				context: OutputContext::Text,
			}),
			token(),
		);
		builder.close();
		builder.close();
		let document = builder.finish();

		let options = PageOptions {
			language: Some("en".into()),
			title: "A & B".into(),
			stylesheets: vec!["style.css".into()],
			icon: None,
		};
		let template = compile(&document, &options);
		let body = "<body>\n<h2 id=\"a&quot;b\" class=\"x y\">1 &lt; 2 &amp; \"3\"</h2>\n\
			<p>See <strong></strong></p>\n</body>";
		assert!(template.content.contains(body), "{}", template.content);
		assert!(
			template
				.content
				.starts_with("<!DOCTYPE html>\n<html lang=\"en\">")
		);
		assert!(template.content.contains("<title>A &amp; B</title>"));
		assert!(
			template
				.content
				.contains("<link rel=\"stylesheet\" href=\"style.css\">")
		);
		assert_eq!(template.slots.len(), 1);
		let position = template.slots[0].position;
		assert_eq!(template.slots[0].slot, slot);
		assert_eq!(&template.content[position..position + 9], "</strong>");
	}

	#[test]
	fn references() {
		let source = Arc::new(Source::new(
			Url::parse("file:///a.nml").unwrap(),
			"x".repeat(10),
			0,
		));
		let token = || Token::new(source.clone(), 0..1);
		let link = |property, context| {
			Slot::Link(LinkSlot {
				target: SymbolRef::any("a"),
				property,
				context,
			})
		};
		let mut builder = DocumentBuilder::new(source.clone());
		builder.open(ElementKind::Paragraph, token());
		let href = builder.add_slot(link(Property::Target, OutputContext::Attribute));
		builder.open(ElementKind::Reference { href }, token());
		let title = builder.slot(link(Property::Title, OutputContext::Text), token());
		builder.close();
		builder.close();
		let document = builder.finish();

		let template = compile(&document, &PageOptions::default());
		let body = "<p><a href=\"\"></a></p>";
		let start = template.content.find(body).unwrap();
		let slots: Vec<_> = template
			.slots
			.iter()
			.map(|s| (s.slot, s.position - start, s.context))
			.collect();
		assert_eq!(
			slots,
			[
				(href, 12, OutputContext::Attribute),
				(title, 14, OutputContext::Text)
			]
		);
	}
}
