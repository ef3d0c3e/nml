use std::collections::HashMap;
use std::collections::HashSet;

use nml_document::Alignment;
use nml_document::Checkbox;
use nml_document::Document;
use nml_document::ElementId;
use nml_document::ElementKind;
use nml_document::LayoutKind;
use nml_document::ListKind;
use nml_document::MediaKind;
use nml_document::OutputContext;
use nml_document::RenderKind;
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
	/// The SVG of the document's rendered elements (math, graphs, …). An element without
	/// one shows its source.
	pub rendered: HashMap<ElementId, String>,
}

/// Compiles a document into a full HTML page.
pub fn compile(document: &Document, options: &PageOptions) -> Template {
	let mut out = Output::new(document, &options.rendered);
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

/// The HTML of a document's content alone, without the page around it. The document must
/// have no slots left to fill.
pub fn fragment(document: &Document) -> String {
	let rendered = HashMap::new();
	let mut out = Output::new(document, &rendered);
	out.element(document, document.root());
	debug_assert!(out.slots.is_empty(), "a fragment has no slots");
	out.content
}

struct Output<'a> {
	content: String,
	slots: Vec<TemplateSlot>,
	rendered: &'a HashMap<ElementId, String>,
	/// The numbers of the enclosing sections, outermost first.
	sections: Vec<u32>,
	/// The ids in the output, so that the ids made for headings are unique.
	ids: HashSet<String>,
}

impl<'a> Output<'a> {
	fn new(document: &Document, rendered: &'a HashMap<ElementId, String>) -> Self {
		let ids = document
			.descendants(document.root())
			.filter_map(|(_, element)| element.attributes.id.clone())
			.collect();
		Self {
			content: String::new(),
			slots: Vec::new(),
			rendered,
			sections: Vec::new(),
			ids,
		}
	}

	/// The number of the next section at `level`, `1.2.`: it closes the deeper sections,
	/// and opens the levels it skips at 1.
	fn section_number(&mut self, level: usize) -> String {
		if self.sections.len() >= level {
			self.sections.truncate(level);
			*self.sections.last_mut().unwrap() += 1;
		} else {
			self.sections.resize(level, 1);
		}
		self.sections.iter().map(|n| format!("{n}.")).collect()
	}

	/// An id made from a heading's text, unlike every other id of the output.
	fn heading_id(&mut self, text: &str) -> String {
		let mut slug = String::new();
		for c in text.chars().flat_map(char::to_lowercase) {
			if c.is_alphanumeric() {
				slug.push(c);
			} else if !slug.is_empty() && !slug.ends_with('-') {
				slug.push('-');
			}
		}
		let slug = match slug.trim_end_matches('-') {
			"" => "section".to_string(),
			slug => slug.to_string(),
		};
		let mut id = slug.clone();
		let mut n = 1;
		while self.ids.contains(&id) {
			n += 1;
			id = format!("{slug}-{n}");
		}
		self.ids.insert(id.clone());
		id
	}

	/// A heading: a link to itself, its number unless it is `unnumbered`, then its content.
	/// A heading without an id gets one from its text.
	fn heading(&mut self, document: &Document, id: ElementId, level: u8) {
		let tag = format!("h{}", level.clamp(1, 6));
		let attributes = &document.get(id).attributes;
		let numbered = !attributes.classes.iter().any(|c| c == "unnumbered");
		let anchor = match &attributes.id {
			Some(anchor) => anchor.clone(),
			None => self.heading_id(&document.text(id)),
		};
		let classes = attributes.classes.join(" ");
		self.raw("<");
		self.raw(&tag);
		self.raw(" id=\"");
		self.attribute(&anchor);
		self.raw("\"");
		if !classes.is_empty() {
			self.raw(" class=\"");
			self.attribute(&classes);
			self.raw("\"");
		}
		self.raw("><a class=\"heading-link\" href=\"#");
		self.attribute(&anchor);
		self.raw("\">🔗</a> ");
		if numbered {
			let number = self.section_number(usize::from(level.max(1)));
			self.raw("<span class=\"heading-number\">");
			self.raw(&number);
			self.raw("</span> ");
		}
		self.children(document, id);
		self.raw("</");
		self.raw(&tag);
		self.raw(">\n");
	}

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
	fn list_item(
		&mut self,
		document: &Document,
		id: ElementId,
		checkbox: Option<Checkbox>,
		value: Option<u64>,
	) {
		let value = value
			.map(|value| format!(" value=\"{value}\""))
			.unwrap_or_default();
		self.open_with(document, id, "li", None, &value);
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

	/// A table: its caption, its header rows in `<thead>`, the others in `<tbody>`.
	fn table(
		&mut self,
		document: &Document,
		id: ElementId,
		alignments: &[Alignment],
		header_rows: usize,
		number: Option<u32>,
	) {
		self.open(document, id, "table");
		self.raw("\n");
		let mut rows = Vec::new();
		for (child, element) in document.children(id) {
			match element.kind {
				ElementKind::Caption => {
					self.open(document, child, "caption");
					if let Some(number) = number {
						self.raw(&format!(
							"<span class=\"table-number\">Table {number}:</span> "
						));
					}
					self.children(document, child);
					self.raw("</caption>\n");
				}
				_ => rows.push(child),
			}
		}
		let (head, body) = rows.split_at(header_rows.min(rows.len()));
		for (section, rows, cell) in [("thead", head, "th"), ("tbody", body, "td")] {
			if rows.is_empty() {
				continue;
			}
			self.raw(&format!("<{section}>\n"));
			// For each column, how many more rows a cell above still covers.
			let mut covered: Vec<u32> = Vec::new();
			for row in rows {
				self.open(document, *row, "tr");
				let mut column = 0;
				for (child, element) in document.children(*row) {
					let ElementKind::TableCell {
						hspan,
						vspan,
						align,
					} = element.kind
					else {
						continue;
					};
					while covered.get(column).is_some_and(|&rows| rows > 0) {
						column += 1;
					}
					let end = column + hspan as usize;
					if covered.len() < end {
						covered.resize(end, 0);
					}
					for rows in &mut covered[column..end] {
						*rows = vspan;
					}
					let mut extra = String::new();
					if hspan > 1 {
						extra.push_str(&format!(" colspan=\"{hspan}\""));
					}
					if vspan > 1 {
						extra.push_str(&format!(" rowspan=\"{vspan}\""));
					}
					let align = match align {
						Alignment::Default => alignments.get(column).copied(),
						align => Some(align),
					};
					let align = match align {
						Some(Alignment::Left) => Some("left"),
						Some(Alignment::Center) => Some("center"),
						Some(Alignment::Right) => Some("right"),
						_ => None,
					};
					if let Some(align) = align {
						extra.push_str(&format!(" style=\"text-align: {align}\""));
					}
					self.open_with(document, child, cell, None, &extra);
					self.children(document, child);
					self.raw(&format!("</{cell}>"));
					column = end;
				}
				for rows in &mut covered {
					*rows = rows.saturating_sub(1);
				}
				self.raw("</tr>\n");
			}
			self.raw(&format!("</{section}>\n"));
		}
		self.raw("</table>\n");
	}

	/// A layout. The styles that make it work are inline, so that it needs no stylesheet.
	fn layout(&mut self, document: &Document, id: ElementId, kind: LayoutKind) {
		let class = format!("layout {}", kind.name());
		let mut children = document.children(id).peekable();
		let title = children
			.next_if(|(_, element)| element.kind == ElementKind::Caption)
			.map(|(title, _)| title);
		let part_style = match kind {
			LayoutKind::Spoiler => {
				self.open_with(document, id, "details", Some(&class), "");
				self.raw("<summary>");
				match title {
					Some(title) => self.children(document, title),
					None => self.text("Spoiler"),
				}
				self.raw("</summary>\n");
				""
			}
			LayoutKind::Centered => {
				self.open_with(
					document,
					id,
					"div",
					Some(&class),
					" style=\"text-align: center\"",
				);
				self.raw("\n");
				""
			}
			LayoutKind::Split => {
				let style = " style=\"display: flex; gap: 1em\"";
				self.open_with(document, id, "div", Some(&class), style);
				self.raw("\n");
				" style=\"flex: 1\""
			}
		};
		for (part, _) in children {
			self.open_with(document, part, "div", Some("part"), part_style);
			self.raw("\n");
			self.children(document, part);
			self.raw("</div>\n");
		}
		self.raw(match kind {
			LayoutKind::Spoiler => "</details>\n",
			_ => "</div>\n",
		});
	}

	/// A quotation, in a figure with its source if it has one.
	fn quote(
		&mut self,
		document: &Document,
		id: ElementId,
		author: Option<&str>,
		cite: Option<&str>,
		url: Option<&str>,
	) {
		let sourced = author.is_some() || cite.is_some();
		if sourced {
			self.raw("<figure class=\"quote\">\n");
		}
		let mut extra = String::new();
		if let Some(url) = url {
			extra.push_str(" cite=\"");
			escape(&mut extra, url, true);
			extra.push('"');
		}
		self.open_with(document, id, "blockquote", None, &extra);
		self.raw("\n");
		self.children(document, id);
		self.raw("</blockquote>\n");
		if sourced {
			self.raw("<figcaption>");
			if let Some(author) = author {
				self.raw("— ");
				self.text(author);
			}
			if let Some(cite) = cite {
				if author.is_some() {
					self.raw(", ");
				}
				self.raw("<cite>");
				self.text(cite);
				self.raw("</cite>");
			}
			self.raw("</figcaption>\n</figure>\n");
		}
	}

	/// A code block: its title, the line numbers in a gutter, and the highlighted code.
	fn code_block(
		&mut self,
		document: &Document,
		id: ElementId,
		language: Option<&str>,
		code: &str,
		first_line: Option<i64>,
		max_lines: Option<u32>,
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
		match max_lines {
			// The line numbers scroll with the code.
			Some(lines) => self.raw(&format!(
				"<div class=\"code\" style=\"max-height: {lines}lh; overflow-y: auto\">"
			)),
			None => self.raw("<div class=\"code\">"),
		}
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
			Slot::Markup(_) => OutputContext::Markup,
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
			// A paragraph of blocks only, such as the output of a script, is its blocks.
			ElementKind::Paragraph
				if document
					.children(id)
					.all(|(_, child)| child.kind.is_block()) =>
			{
				self.children(document, id);
			}
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
			ElementKind::Heading { level } => self.heading(document, id, *level),
			ElementKind::Slot(slot) => match document.slot(*slot) {
				Slot::Markup(markup) => self.raw(markup),
				_ => self.slot(document, *slot),
			},
			ElementKind::Reference { href } => {
				self.raw("<a href=\"");
				self.slot(document, *href);
				self.raw("\">");
				self.children(document, id);
				self.raw("</a>");
			}
			ElementKind::Code { code, language } => match language {
				Some(language) => {
					let mut extra = String::from(" data-language=\"");
					escape(&mut extra, language, true);
					extra.push('"');
					self.open_with(document, id, "code", Some("highlighted"), &extra);
					self.raw(&nml_render::highlight(code, Some(language)));
					self.raw("</code>");
				}
				None => {
					self.open(document, id, "code");
					self.text(code);
					self.raw("</code>");
				}
			},
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
				max_lines,
			} => self.code_block(
				document,
				id,
				language.as_deref(),
				code,
				*first_line,
				*max_lines,
			),
			ElementKind::Media {
				kind,
				url,
				alt,
				width,
				number,
			} => {
				let mut extra = String::from(" src=\"");
				escape(&mut extra, url, true);
				extra.push('"');
				if *kind == MediaKind::Image {
					extra.push_str(" alt=\"");
					escape(&mut extra, alt, true);
					extra.push('"');
				} else {
					extra.push_str(" controls");
				}
				if let Some(width) = width {
					extra.push_str(" style=\"width: ");
					escape(&mut extra, width, true);
					extra.push('"');
				}
				let tag = match kind {
					MediaKind::Image => "img",
					MediaKind::Video => "video",
					MediaKind::Audio => "audio",
				};
				self.open_with(document, id, tag, None, &extra);
				if *kind != MediaKind::Image {
					// Shown where the medium cannot be played.
					self.text(alt);
					self.raw("</");
					self.raw(tag);
					self.raw(">");
				}
				if let Some(number) = number {
					self.raw(&format!("<span class=\"media-number\">({number})</span>"));
				}
			}
			ElementKind::Figure { number } => {
				self.open_with(document, id, "figure", None, "");
				self.raw("\n");
				for (child, element) in document.children(id) {
					match &element.kind {
						// A paragraph that holds the figure's media is not a paragraph of text.
						ElementKind::Paragraph => {
							self.children(document, child);
							self.raw("\n");
						}
						ElementKind::Caption => {
							self.open(document, child, "figcaption");
							self.raw(&format!(
								"<span class=\"figure-number\">Figure {number}:</span> "
							));
							self.children(document, child);
							self.raw("</figcaption>\n");
						}
						_ => self.element(document, child),
					}
				}
				self.raw("</figure>\n");
			}
			ElementKind::Caption => self.wrap(document, id, "figcaption"),
			ElementKind::Quote { author, cite, url } => self.quote(
				document,
				id,
				author.as_deref(),
				cite.as_deref(),
				url.as_deref(),
			),
			ElementKind::Callout(kind) => {
				self.open_with(
					document,
					id,
					"aside",
					Some(&format!("callout {}", kind.name())),
					"",
				);
				self.raw("\n<p class=\"callout-title\">");
				let mut children = document.children(id).peekable();
				match children.peek() {
					Some((title, element)) if element.kind == ElementKind::Caption => {
						self.children(document, *title);
						children.next();
					}
					// The kind is the default title: `Note`.
					_ => {
						let name = kind.name();
						self.text(&name[..1].to_uppercase());
						self.text(&name[1..]);
					}
				}
				self.raw("</p>\n");
				for (child, _) in children {
					self.element(document, child);
				}
				self.raw("</aside>\n");
			}
			ElementKind::Layout(kind) => self.layout(document, id, *kind),
			ElementKind::LayoutPart => {
				self.wrap(document, id, "div");
				self.raw("\n");
			}
			ElementKind::Table {
				alignments,
				header_rows,
				number,
			} => self.table(document, id, alignments, *header_rows, *number),
			ElementKind::TableRow | ElementKind::TableCell { .. } => {
				unreachable!("rows and cells are written with their table")
			}
			ElementKind::Rendered {
				kind,
				source,
				block,
			} => {
				let tag = if *block { "div" } else { "span" };
				let mut extra = String::from(" role=\"img\" aria-label=\"");
				escape(&mut extra, source, true);
				extra.push('"');
				let width = match kind {
					RenderKind::Graph { width, .. } => width.as_deref(),
					_ => None,
				};
				if let Some(width) = width {
					extra.push_str(" style=\"width: ");
					escape(&mut extra, width, true);
					extra.push('"');
				}
				let class = format!("rendered {}", kind.name());
				self.open_with(document, id, tag, Some(&class), &extra);
				match self.rendered.get(&id) {
					// A sized image fills its box: CSS beats the SVG's own size.
					Some(svg) if width.is_some() => self.raw(&svg.replacen(
						"<svg",
						"<svg style=\"width: 100%; height: auto\"",
						1,
					)),
					Some(svg) => self.raw(svg),
					None => {
						self.raw("<code>");
						self.text(source);
						self.raw("</code>");
					}
				}
				self.raw(&format!("</{tag}>"));
				if *block {
					self.raw("\n");
				}
			}
			ElementKind::Anchor => {
				self.wrap(document, id, "span");
				self.raw("\n");
			}
			ElementKind::Raw { content, block } => {
				self.raw(content);
				if *block {
					self.raw("\n");
				}
			}
			ElementKind::List(kind) => {
				let (tag, extra) = match kind {
					ListKind::Bullet { style } => {
						let style = style
							.map(|style| format!(" style=\"list-style-type: {}\"", style.name()));
						("ul", style.unwrap_or_default())
					}
					ListKind::Numbered { start, style } => {
						let mut extra = String::new();
						if *start != 1 {
							extra.push_str(&format!(" start=\"{start}\""));
						}
						if let Some(style) = style {
							extra.push_str(&format!(" type=\"{}\"", style.name()));
						}
						("ol", extra)
					}
				};
				self.open_with(document, id, tag, None, &extra);
				self.raw("\n");
				self.children(document, id);
				self.raw("</");
				self.raw(tag);
				self.raw(">\n");
			}
			ElementKind::ListItem { checkbox, value } => {
				self.list_item(document, id, *checkbox, *value)
			}
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
			..PageOptions::default()
		};
		let template = compile(&document, &options);
		let body = "<body>\n<h2 id=\"a&quot;b\" class=\"x y\"><a class=\"heading-link\" href=\"#a&quot;b\">🔗</a> \
			<span class=\"heading-number\">1.1.</span> 1 &lt; 2 &amp; \"3\"</h2>\n\
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
