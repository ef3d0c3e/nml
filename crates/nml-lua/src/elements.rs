//! Elements built by scripts with `nml.element`, and the values scripts show: strings and
//! numbers are text, elements are markup, and lists hold several of them.

use std::sync::Arc;

use mlua::AnyUserData;
use mlua::Lua;
use mlua::Table;
use mlua::UserData;
use mlua::UserDataFields;
use mlua::Value;
use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::Alignment;
use nml_document::Attributes;
use nml_document::BulletStyle;
use nml_document::CalloutKind;
use nml_document::Checkbox;
use nml_document::DocumentBuilder;
use nml_document::ElementKind;
use nml_document::Export;
use nml_document::LayoutKind;
use nml_document::ListKind;
use nml_document::MediaKind;
use nml_document::NumberStyle;
use nml_document::RenderKind;
use nml_document::Scope;
use nml_document::Style;
use nml_document::SymbolKind;
use nml_document::SymbolRef;
use nml_lang::Lower;
use nml_source::Source;
use nml_source::Token;
use nml_source::Url;

use crate::api;

/// An element built by a script, with its children.
#[derive(Debug, Clone)]
pub(crate) struct Element {
	content: Content,
	attributes: Attributes,
	children: Vec<Element>,
}

#[derive(Debug, Clone)]
enum Content {
	Element(ElementKind),
	/// A reference, whose link slots are made when it is written into a document.
	Reference(SymbolRef),
	/// A figure, numbered when it is written into a document. Its caption is written after
	/// its body.
	Figure {
		reference: Option<String>,
		caption: Vec<Element>,
	},
	/// Markup, parsed with the unit's grammar when it is written into a document.
	Markup(String),
	/// An anchor: a reference target, declared when it is written into a document.
	Anchor {
		reference: String,
		title: Option<String>,
	},
	/// A table, numbered when it is written into a document if it has a reference or a
	/// caption. Its children are its rows.
	Table {
		reference: Option<String>,
		caption: Vec<Element>,
		alignments: Vec<Alignment>,
		header: usize,
	},
}

impl UserData for Element {
	fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
		fields.add_field_method_get("kind", |_, element| {
			Ok(match &element.content {
				Content::Element(kind) => kind.name(),
				Content::Reference(_) => "reference",
				Content::Figure { .. } => "figure",
				Content::Table { .. } => "table",
				Content::Anchor { .. } => "anchor",
				Content::Markup(_) => "markup",
			})
		});
	}
}

/// The declaration of [`Element`] for LuaLS.
pub(crate) const NODE_CLASS: crate::LuaClass = crate::LuaClass {
	name: "nml.Node",
	doc: "An element built with `nml.element`.",
	fields: &[crate::LuaField {
		name: "kind",
		ty: "string",
		doc: "The element's kind, as element events name it.",
		readonly: true,
	}],
};

/// The kinds `nml.element` builds, and the fields each one reads.
const KINDS: &[(&str, &str)] = &[
	("paragraph", ""),
	("bold", ""),
	("italic", ""),
	("underline", ""),
	("highlight", ""),
	("strikethrough", ""),
	("heading", "level"),
	("link", "url"),
	("reference", "target"),
	("code", "code, language"),
	(
		"code_block",
		"code, language, line_offset, numbers, max_lines",
	),
	("media", "url, type, alt, width"),
	("figure", "caption, ref"),
	("list", "numbered, start, bullet"),
	("list_item", "checkbox, value"),
	("quote", "author, cite, url"),
	("callout", "type, title"),
	("layout", "type, title"),
	("raw", "content, block"),
	("table", "caption, ref, header, align"),
	("cell", "hspan, vspan, align"),
	("anchor", "ref, title"),
	("math", "source, block"),
	("tex", "source, env"),
	("graph", "source, layout, width"),
	("typst", "source, block"),
];

/// Builds an element. The list part of `spec` holds its children (strings, numbers,
/// elements, or lists of them), and its fields the element's properties, by kind:
///
/// | Kind | Fields |
/// |---|---|
/// | `paragraph`, `bold`, `italic`, `underline`, `highlight`, `strikethrough` | |
/// | `heading` | `level` (1 to 6) |
/// | `link` | `url` |
/// | `reference` | `target` (`name`, `doc#name`, `doc#`, `#name`); children are the caption |
/// | `code` | `code`, `language` (highlighted in it) |
/// | `code_block` | `code`, `language`, `line_offset`, `numbers`, `max_lines`; children are the title |
/// | `media` | `url` (a path in the workspace, copied with the page, or an external URL), `type` (`image`, `video`, `audio`), `alt`, `width` |
/// | `figure` | `caption`, `ref` (its reference name, and its id); children are the body. Numbered with the unit's other figures |
/// | `list` | `numbered`, `start`, `bullet` (`1`, `a`, `A`, `i`, `I` if numbered; `disc`, `circle`, `square`, `none` if not); children that are not items become items |
/// | `list_item` | `checkbox` (`unchecked`, `partial`, `checked`), `value` (its number) |
/// | `quote` | `author`, `cite`, `url` |
/// | `callout` | `type` (`note`, `tip`, …), `title` |
/// | `layout` | `type` (`centered`, `split`, `spoiler`), `title`; each child is a part |
/// | `raw` | `content` (HTML, written as is), `block` |
/// | `table` | `caption`, `ref` (its reference name, and its id), `header` (how many rows are header rows), `align` (each column's: `auto`, `left`, `center`, `right`); children are the rows, each a list of cells. Numbered with the unit's other tables if it has a `ref` or a `caption` |
/// | `cell` | `hspan`, `vspan`, `align`; a cell of a table's row, for a cell that needs these. Any other value in a row is a cell's content |
/// | `anchor` | `ref` (its reference name, and its id), `title` |
/// | `math` | `source` (LaTeX math), `block` (displayed) |
/// | `tex` | `source` (a LaTeX document body), `env` (an environment of `[render.tex]`, `main` by default) |
/// | `graph` | `source` (Graphviz DOT), `layout` (`dot` by default), `width` |
/// | `typst` | `source`, `block` |
///
/// Every kind takes `id` and `classes`. References, figures, numbered tables, anchors and
/// rendered images (`math`, `tex`, `graph`, `typst`) only work where the element goes into
/// the document as it is parsed (roles and rules), not in post-parse or deferred output;
/// nor do media with paths in deferred output.
#[api(returns = "nml.Node")]
fn element(lua: &Lua, kind: String, spec: Option<Table>) -> mlua::Result<AnyUserData> {
	let spec = match spec {
		Some(spec) => spec,
		None => lua.create_table()?,
	};
	lua.create_userdata(build(&kind, &spec)?)
}

/// Markup: `text` is parsed as inline content, with the page's grammar and variables, where
/// the result goes into the document. A string a script returns is text; this is how a
/// script writes markup instead.
///
/// Errors in `text` are reported where the script is. Like references, it only works where
/// the element goes into the document as it is parsed, not in post-parse or deferred output.
#[api(returns = "nml.Node")]
fn parse(lua: &Lua, text: String) -> mlua::Result<AnyUserData> {
	lua.create_userdata(Element {
		content: Content::Markup(text),
		attributes: Attributes::default(),
		children: Vec::new(),
	})
}

pub(crate) fn build(kind: &str, spec: &Table) -> mlua::Result<Element> {
	let text = |name: &str| spec.get::<Option<String>>(name);
	let required = |name: &str| {
		text(name)?.ok_or_else(|| {
			mlua::Error::runtime(format!("a `{kind}` element needs the field `{name}`"))
		})
	};
	let mut children = nodes(Value::Table(spec.clone()))?;
	let content = match kind {
		"paragraph" => Content::Element(ElementKind::Paragraph),
		"bold" => Content::Element(ElementKind::Style(Style::Bold)),
		"italic" => Content::Element(ElementKind::Style(Style::Italic)),
		"underline" => Content::Element(ElementKind::Style(Style::Underline)),
		"highlight" => Content::Element(ElementKind::Style(Style::Highlight)),
		"strikethrough" => Content::Element(ElementKind::Style(Style::Strikethrough)),
		"heading" => {
			let level: u8 = spec.get::<Option<u8>>("level")?.unwrap_or(0);
			if !(1..=6).contains(&level) {
				return Err(mlua::Error::runtime("a heading's `level` is 1 to 6"));
			}
			Content::Element(ElementKind::Heading { level })
		}
		"link" => Content::Element(ElementKind::Link {
			url: required("url")?,
		}),
		"reference" => {
			let target = required("target")?;
			let target = SymbolRef::parse(&target)
				.ok_or_else(|| mlua::Error::runtime(format!("`{target}` is not a reference")))?;
			Content::Reference(target)
		}
		"code" => Content::Element(ElementKind::Code {
			code: required("code")?,
			language: text("language")?,
		}),
		"code_block" => {
			let numbered = spec.get::<Option<bool>>("numbers")?.unwrap_or(true);
			let first = spec.get::<Option<i64>>("line_offset")?.unwrap_or(1);
			let max_lines = match spec.get::<Option<i64>>("max_lines")? {
				None | Some(0) => None,
				Some(lines) if lines > 0 => Some(u32::try_from(lines).unwrap_or(u32::MAX)),
				Some(_) => {
					return Err(mlua::Error::runtime(
						"a code block's `max_lines` is positive, or 0 for no limit",
					));
				}
			};
			Content::Element(ElementKind::CodeBlock {
				language: text("language")?,
				code: required("code")?,
				first_line: numbered.then_some(first),
				max_lines,
			})
		}
		"media" => {
			let url = required("url")?;
			let kind = match text("type")?.as_deref() {
				Some("image") => Some(MediaKind::Image),
				Some("video") => Some(MediaKind::Video),
				Some("audio") => Some(MediaKind::Audio),
				Some(other) => {
					return Err(mlua::Error::runtime(format!(
						"`{other}` is not a type of media: use image, video or audio"
					)));
				}
				None => MediaKind::from_path(&url),
			};
			let kind = kind.ok_or_else(|| {
				mlua::Error::runtime(format!("set the `type` of `{url}`: image, video or audio"))
			})?;
			Content::Element(ElementKind::Media {
				kind,
				url,
				alt: text("alt")?.unwrap_or_default(),
				width: text("width")?,
				number: None,
			})
		}
		"list" => {
			let bullet = text("bullet")?;
			let unknown = |bullet: &str, styles: Vec<&str>| {
				mlua::Error::runtime(format!(
					"`{bullet}` is not a bullet of this list: use one of {}",
					styles.join(", ")
				))
			};
			let kind = match spec.get::<Option<bool>>("numbered")?.unwrap_or(false) {
				true => ListKind::Numbered {
					start: spec.get::<Option<u64>>("start")?.unwrap_or(1),
					style: match bullet {
						Some(bullet) => Some(NumberStyle::from_name(&bullet).ok_or_else(|| {
							unknown(&bullet, NumberStyle::ALL.iter().map(|s| s.name()).collect())
						})?),
						None => None,
					},
				},
				false => ListKind::Bullet {
					style: match bullet {
						Some(bullet) => Some(BulletStyle::from_name(&bullet).ok_or_else(|| {
							unknown(&bullet, BulletStyle::ALL.iter().map(|s| s.name()).collect())
						})?),
						None => None,
					},
				},
			};
			children = children
				.into_iter()
				.map(|child| match child.content {
					Content::Element(ElementKind::ListItem { .. }) => child,
					_ => {
						let item = ElementKind::ListItem {
							checkbox: None,
							value: None,
						};
						wrap(item, child)
					}
				})
				.collect();
			Content::Element(ElementKind::List(kind))
		}
		"list_item" => {
			let checkbox = match text("checkbox")?.as_deref() {
				None => None,
				Some("unchecked") => Some(Checkbox::Unchecked),
				Some("partial") => Some(Checkbox::Partial),
				Some("checked") => Some(Checkbox::Checked),
				Some(other) => {
					return Err(mlua::Error::runtime(format!(
						"`{other}` is not a checkbox: use unchecked, partial or checked"
					)));
				}
			};
			Content::Element(ElementKind::ListItem {
				checkbox,
				value: spec.get::<Option<u64>>("value")?,
			})
		}
		"quote" => Content::Element(ElementKind::Quote {
			author: text("author")?,
			cite: text("cite")?,
			url: text("url")?,
		}),
		"callout" => {
			let name = required("type")?;
			let kind = CalloutKind::from_name(&name).ok_or_else(|| {
				mlua::Error::runtime(format!("`{name}` is not a kind of callout"))
			})?;
			if let Some(title) = text("title")? {
				children.insert(0, caption(title));
			}
			Content::Element(ElementKind::Callout(kind))
		}
		"layout" => {
			let name = required("type")?;
			let kind = LayoutKind::from_name(&name)
				.ok_or_else(|| mlua::Error::runtime(format!("`{name}` is not a layout")))?;
			children = children
				.into_iter()
				.map(|child| wrap(ElementKind::LayoutPart, child))
				.collect();
			if let Some(expected) = kind.parts()
				&& children.len() != expected
			{
				return Err(mlua::Error::runtime(format!(
					"the `{name}` layout has {expected} part(s), not {}",
					children.len()
				)));
			}
			if let Some(title) = text("title")? {
				children.insert(0, caption(title));
			}
			Content::Element(ElementKind::Layout(kind))
		}
		"figure" => {
			children = paragraphs(children);
			Content::Figure {
				reference: reference_name(kind, spec)?,
				caption: nodes(spec.get::<Value>("caption")?)?,
			}
		}
		"table" => {
			children = rows(spec)?;
			let header = spec.get::<Option<usize>>("header")?.unwrap_or(0);
			if header > children.len() {
				return Err(mlua::Error::runtime(format!(
					"a table of {} row(s) cannot have {header} header row(s)",
					children.len()
				)));
			}
			let alignments = spec
				.get::<Option<Vec<String>>>("align")?
				.unwrap_or_default()
				.iter()
				.map(|name| alignment(name))
				.collect::<mlua::Result<_>>()?;
			Content::Table {
				reference: reference_name(kind, spec)?,
				caption: nodes(spec.get::<Value>("caption")?)?,
				alignments,
				header,
			}
		}
		"anchor" => Content::Anchor {
			reference: reference_name(kind, spec)?
				.ok_or_else(|| mlua::Error::runtime("an `anchor` element needs the field `ref`"))?,
			title: text("title")?,
		},
		"math" | "typst" => Content::Element(ElementKind::Rendered {
			kind: match kind {
				"math" => RenderKind::Math,
				_ => RenderKind::Typst,
			},
			source: required("source")?,
			block: spec.get::<Option<bool>>("block")?.unwrap_or(false),
		}),
		"tex" => Content::Element(ElementKind::Rendered {
			kind: RenderKind::Tex {
				environment: text("env")?.unwrap_or_else(|| "main".into()),
			},
			source: required("source")?,
			block: true,
		}),
		"graph" => {
			let layout = text("layout")?.unwrap_or_else(|| "dot".into());
			if !nml_lang::GRAPH_LAYOUTS.contains(&layout.as_str()) {
				return Err(mlua::Error::runtime(format!(
					"`{layout}` is not a Graphviz layout: use one of {}",
					nml_lang::GRAPH_LAYOUTS.join(", ")
				)));
			}
			Content::Element(ElementKind::Rendered {
				kind: RenderKind::Graph {
					layout,
					width: text("width")?,
				},
				source: required("source")?,
				block: true,
			})
		}
		"cell" => {
			let span = |name: &str| -> mlua::Result<u32> {
				match spec.get::<Option<i64>>(name)? {
					None => Ok(1),
					Some(span) if span >= 1 => Ok(u32::try_from(span).unwrap_or(u32::MAX)),
					Some(_) => Err(mlua::Error::runtime(format!(
						"a cell's `{name}` is at least 1"
					))),
				}
			};
			let align = match text("align")? {
				Some(name) => alignment(&name)?,
				None => Alignment::Default,
			};
			Content::Element(ElementKind::TableCell {
				hspan: span("hspan")?,
				vspan: span("vspan")?,
				align,
			})
		}
		"raw" => Content::Element(ElementKind::Raw {
			content: required("content")?,
			block: spec.get::<Option<bool>>("block")?.unwrap_or(false),
		}),
		_ => {
			let kinds: Vec<_> = KINDS.iter().map(|(kind, _)| *kind).collect();
			return Err(mlua::Error::runtime(format!(
				"no element kind is named `{kind}`: use one of {}",
				kinds.join(", ")
			)));
		}
	};
	let id = match &content {
		Content::Figure { reference, .. } | Content::Table { reference, .. } => reference.clone(),
		Content::Anchor { reference, .. } => Some(reference.clone()),
		_ => text("id")?,
	};
	let attributes = Attributes {
		id,
		classes: spec
			.get::<Option<Vec<String>>>("classes")?
			.unwrap_or_default(),
	};
	Ok(Element {
		content,
		attributes,
		children,
	})
}

/// The reference name in `ref` of a figure or table, which is also its id.
fn reference_name(kind: &str, spec: &Table) -> mlua::Result<Option<String>> {
	let Some(name) = spec.get::<Option<String>>("ref")? else {
		return Ok(None);
	};
	let valid = SymbolRef::parse(&name)
		.is_some_and(|symbol| symbol.scope == Scope::Local && symbol.name == name);
	if !valid {
		return Err(mlua::Error::runtime(format!(
			"`{name}` is not a reference name"
		)));
	}
	if spec.get::<Option<String>>("id")?.is_some() {
		return Err(mlua::Error::runtime(format!(
			"a {kind}'s `ref` is its id: it cannot have an `id` too"
		)));
	}
	Ok(Some(name))
}

/// The rows of a table: the list part of `spec`, each a list of cells.
fn rows(spec: &Table) -> mlua::Result<Vec<Element>> {
	let mut rows = Vec::new();
	for row in spec.sequence_values::<Value>() {
		let Value::Table(row) = row? else {
			return Err(mlua::Error::runtime("a table's rows are lists of cells"));
		};
		let mut cells = Vec::new();
		for cell in row.sequence_values::<Value>() {
			let cell = cell?;
			if let Value::UserData(data) = &cell
				&& let Ok(element) = data.borrow::<Element>()
				&& matches!(
					element.content,
					Content::Element(ElementKind::TableCell { .. })
				) {
				cells.push(element.clone());
				continue;
			}
			cells.push(Element {
				children: nodes(cell)?,
				..leaf(ElementKind::TableCell {
					hspan: 1,
					vspan: 1,
					align: Alignment::Default,
				})
			});
		}
		rows.push(Element {
			children: cells,
			..leaf(ElementKind::TableRow)
		});
	}
	Ok(rows)
}

fn alignment(name: &str) -> mlua::Result<Alignment> {
	match name {
		"auto" => Ok(Alignment::Default),
		"left" => Ok(Alignment::Left),
		"center" => Ok(Alignment::Center),
		"right" => Ok(Alignment::Right),
		_ => Err(mlua::Error::runtime(format!(
			"`{name}` is not an alignment: use auto, left, center or right"
		))),
	}
}

/// Blocks, with the inline elements between them gathered into paragraphs.
fn paragraphs(elements: Vec<Element>) -> Vec<Element> {
	let mut blocks = Vec::new();
	let mut inline = Vec::new();
	let flush = |inline: &mut Vec<Element>, blocks: &mut Vec<Element>| {
		if !inline.is_empty() {
			blocks.push(Element {
				children: std::mem::take(inline),
				..leaf(ElementKind::Paragraph)
			});
		}
	};
	for element in elements {
		let block = match &element.content {
			Content::Element(kind) => kind.is_block(),
			Content::Reference(_) | Content::Anchor { .. } | Content::Markup(_) => false,
			Content::Figure { .. } | Content::Table { .. } => true,
		};
		if block {
			flush(&mut inline, &mut blocks);
			blocks.push(element);
		} else {
			inline.push(element);
		}
	}
	flush(&mut inline, &mut blocks);
	blocks
}

fn leaf(kind: ElementKind) -> Element {
	Element {
		content: Content::Element(kind),
		attributes: Attributes::default(),
		children: Vec::new(),
	}
}

fn wrap(kind: ElementKind, child: Element) -> Element {
	Element {
		children: vec![child],
		..leaf(kind)
	}
}

fn caption(title: String) -> Element {
	wrap(ElementKind::Caption, leaf(ElementKind::Text(title)))
}

/// The elements a value stands for: text for strings and numbers, elements, and the items
/// of a list, in order. `nil` is nothing.
pub(crate) fn nodes(value: Value) -> mlua::Result<Vec<Element>> {
	let mut out = Vec::new();
	collect(value, &mut out)?;
	Ok(out)
}

fn collect(value: Value, out: &mut Vec<Element>) -> mlua::Result<()> {
	match value {
		Value::Nil => {}
		Value::String(text) => out.push(leaf(ElementKind::Text(text.to_str()?.to_string()))),
		Value::Integer(n) => out.push(leaf(ElementKind::Text(n.to_string()))),
		Value::Number(n) => out.push(leaf(ElementKind::Text(number_text(n)))),
		Value::UserData(data) if data.is::<Element>() => {
			out.push(data.borrow::<Element>()?.clone());
		}
		Value::Table(table) => {
			for value in table.sequence_values::<Value>() {
				collect(value?, out)?;
			}
		}
		other => {
			return Err(mlua::Error::runtime(format!(
				"expected text, an element or a list of them, not a {}",
				other.type_name()
			)));
		}
	}
	Ok(())
}

/// A float as Lua shows it.
pub(crate) fn number_text(n: f64) -> String {
	if n.fract() == 0.0 && n.is_finite() {
		format!("{n:.1}")
	} else {
		n.to_string()
	}
}

/// Writes elements into the document being lowered, at `token`.
pub(crate) fn lower(lower: &mut Lower<'_, '_>, elements: &[Element], token: &Token) {
	for element in elements {
		let opened = open(lower, element, token);
		close(lower, opened, token);
	}
}

/// What closing an element opened with [`open`] still has to do.
pub(crate) struct Opened<'e> {
	figure: Option<OpenFigure<'e>>,
	table: Option<OpenTable<'e>>,
	/// An anchor's reference name and title.
	anchor: Option<(&'e str, &'e str)>,
	/// Whether nothing was opened: markup is written as it is parsed.
	nothing: bool,
}

/// A numbered table being written.
struct OpenTable<'e> {
	number: u32,
	reference: Option<&'e str>,
	caption: &'e [Element],
}

struct OpenFigure<'e> {
	number: u32,
	reference: Option<&'e str>,
	caption: &'e [Element],
	/// The number of the unit's assets when the figure was opened: the first one after is
	/// the medium it shows.
	assets: usize,
}

/// Opens an element in the document being lowered, and writes its children. The caller
/// closes it with [`close`].
pub(crate) fn open<'e>(
	lower: &mut Lower<'_, '_>,
	element: &'e Element,
	token: &Token,
) -> Opened<'e> {
	let mut figure = None;
	let mut table = None;
	let mut anchor = None;
	match &element.content {
		Content::Element(ElementKind::Media {
			kind,
			url,
			alt,
			width,
			..
		}) => {
			let url = match nml_lang::media_url(lower, url) {
				Ok(url) => url,
				Err(_) => {
					let kind = DiagnosticKind::FileNotFound { path: url.clone() };
					let message = DiagnosticMessage::new(token.clone());
					lower.diagnostics.push(Diagnostic::new(kind, message));
					url.clone()
				}
			};
			let kind = ElementKind::Media {
				kind: *kind,
				url,
				alt: alt.clone(),
				width: width.clone(),
				number: None,
			};
			lower
				.builder
				.open_with(kind, token.clone(), element.attributes.clone());
		}
		Content::Element(kind) => {
			lower
				.builder
				.open_with(kind.clone(), token.clone(), element.attributes.clone());
		}
		Content::Reference(target) => {
			lower.open_reference(target.clone(), token.clone(), token.clone());
			if element.children.is_empty() {
				lower.reference_label(target.clone(), token.clone());
				return Opened {
					figure,
					table,
					anchor,
					nothing: false,
				};
			}
		}
		Content::Figure { reference, caption } => {
			let number = nml_lang::next_figure(lower);
			lower.builder.open_with(
				ElementKind::Figure { number },
				token.clone(),
				element.attributes.clone(),
			);
			figure = Some(OpenFigure {
				number,
				reference: reference.as_deref(),
				caption,
				assets: lower.assets.len(),
			});
		}
		Content::Markup(text) => {
			lower_markup(lower, text, token);
			return Opened {
				figure,
				table,
				anchor,
				nothing: true,
			};
		}
		Content::Anchor { reference, title } => {
			lower.builder.open_with(
				ElementKind::Anchor,
				token.clone(),
				element.attributes.clone(),
			);
			anchor = Some((reference.as_str(), title.as_deref().unwrap_or(reference)));
		}
		Content::Table {
			reference,
			caption,
			alignments,
			header,
		} => {
			let numbered = reference.is_some() || !caption.is_empty();
			let number = numbered.then(|| nml_lang::next_table(lower));
			let kind = ElementKind::Table {
				alignments: alignments.clone(),
				header_rows: *header,
				number,
			};
			lower
				.builder
				.open_with(kind, token.clone(), element.attributes.clone());
			if !caption.is_empty() {
				lower.builder.open(ElementKind::Caption, token.clone());
				self::lower(lower, caption, token);
				lower.builder.close();
			}
			table = number.map(|number| OpenTable {
				number,
				reference: reference.as_deref(),
				caption,
			});
		}
	}
	self::lower(lower, &element.children, token);
	Opened {
		figure,
		table,
		anchor,
		nothing: false,
	}
}

/// Parses markup from a script and writes it. Its source is virtual: its positions all map
/// to `token`, where the script is.
fn lower_markup(lower: &mut Lower<'_, '_>, text: &str, token: &Token) {
	let (root, offset) = token
		.source()
		.root_offset(token.start(), nml_source::Bias::Start);
	let mut location = Url::parse("virtual:///").unwrap();
	location.set_path(&format!(
		"{}/script-markup/{offset}",
		root.location().path()
	));
	let mut builder =
		nml_source::VirtualSourceBuilder::new(token.source().clone(), location, token.start());
	builder.insert(text);
	let source = Arc::new(builder.build());
	let parsed = nml_grammar::parse_inline(lower.language().rules(), source);
	lower.diagnostics.extend(parsed.diagnostics);
	lower.nodes(&parsed.nodes);
}

/// Closes an element opened with [`open`]: a figure gets its caption, and its symbol.
pub(crate) fn close(lower: &mut Lower<'_, '_>, opened: Opened<'_>, token: &Token) {
	if opened.nothing {
		return;
	}
	if let Some((reference, title)) = opened.anchor {
		let element = lower.builder.close();
		let export = Export::new(reference, SymbolKind::Anchor, Some(element)).with("title", title);
		lower.export(export, token.clone());
		return;
	}
	if let Some(table) = opened.table {
		let element = lower.builder.close();
		if let Some(name) = table.reference {
			let title = plain_text(table.caption);
			let export = nml_lang::table_export(name, element, table.number, title);
			lower.export(export, token.clone());
		}
		return;
	}
	let Some(figure) = opened.figure else {
		lower.builder.close();
		return;
	};
	if !figure.caption.is_empty() {
		lower.builder.open(ElementKind::Caption, token.clone());
		self::lower(lower, figure.caption, token);
		lower.builder.close();
	}
	let element = lower.builder.close();
	if let Some(name) = figure.reference {
		let medium = (lower.assets.get(figure.assets)).map(|asset| asset.path.clone());
		let title = plain_text(figure.caption);
		let export = nml_lang::figure_export(name, element, figure.number, title, medium);
		lower.export(export, token.clone());
	}
}

/// The text of elements without markup.
fn plain_text(elements: &[Element]) -> String {
	fn collect(elements: &[Element], out: &mut String) {
		for element in elements {
			if let Content::Element(
				ElementKind::Text(text) | ElementKind::Code { code: text, .. },
			) = &element.content
			{
				out.push_str(text);
			}
			if let Content::Figure { caption, .. } | Content::Table { caption, .. } =
				&element.content
			{
				collect(caption, out);
			}
			if let Content::Markup(text) = &element.content {
				out.push_str(text);
			}
			collect(&element.children, out);
		}
	}
	let mut out = String::new();
	collect(elements, &mut out);
	out
}

/// The HTML of elements, for output made after the parse: post-parse, with the unit being
/// lowered, and deferred, without. It cannot hold references or figures, which need the
/// parse, nor media with paths if there is no unit to add them to.
pub(crate) fn html(
	elements: &[Element],
	mut lower: Option<&mut Lower<'_, '_>>,
) -> mlua::Result<String> {
	let source = Arc::new(Source::new(
		Url::parse("virtual:///script-output").unwrap(),
		String::new(),
		0,
	));
	let token = Token::new(source.clone(), 0..0);
	let mut builder = DocumentBuilder::new(source);
	fn write(
		builder: &mut DocumentBuilder<'_>,
		lower: &mut Option<&mut Lower<'_, '_>>,
		elements: &[Element],
		token: &Token,
	) -> mlua::Result<()> {
		for element in elements {
			let kind = match &element.content {
				Content::Element(ElementKind::Media {
					kind,
					url,
					alt,
					width,
					..
				}) => {
					let url = match lower {
						Some(lower) => nml_lang::media_url(lower, url).map_err(|_| {
							mlua::Error::runtime(format!("`{url}` does not exist in the workspace"))
						})?,
						None if nml_lang::is_external(url) => url.clone(),
						None => {
							return Err(mlua::Error::runtime(format!(
								"`{url}` is a file of the workspace, which needs the parse: \
								 only external media can be shown at link time"
							)));
						}
					};
					ElementKind::Media {
						kind: *kind,
						url,
						alt: alt.clone(),
						width: width.clone(),
						number: None,
					}
				}
				Content::Element(ElementKind::Rendered { kind, .. }) => {
					return Err(mlua::Error::runtime(format!(
						"`{}` is rendered from the parse: it cannot be made after it",
						kind.name()
					)));
				}
				Content::Element(kind) => kind.clone(),
				Content::Reference(_) => {
					return Err(mlua::Error::runtime(
						"a reference needs the parse: it cannot be made after it",
					));
				}
				Content::Figure { .. } => {
					return Err(mlua::Error::runtime(
						"a figure is numbered during the parse: it cannot be made after it",
					));
				}
				Content::Anchor { .. } => {
					return Err(mlua::Error::runtime(
						"an anchor is declared during the parse: it cannot be made after it",
					));
				}
				Content::Markup(_) => {
					return Err(mlua::Error::runtime(
						"markup from `nml.parse` needs the parse: it cannot be shown after it",
					));
				}
				Content::Table {
					reference,
					caption,
					alignments,
					header,
				} => {
					if reference.is_some() || !caption.is_empty() {
						return Err(mlua::Error::runtime(
							"a table with a `ref` or a `caption` is numbered during the parse: \
							 it cannot be made after it",
						));
					}
					ElementKind::Table {
						alignments: alignments.clone(),
						header_rows: *header,
						number: None,
					}
				}
			};
			builder.open_with(kind, token.clone(), element.attributes.clone());
			write(builder, lower, &element.children, token)?;
			builder.close();
		}
		Ok(())
	}
	write(&mut builder, &mut lower, elements, &token)?;
	Ok(nml_html::fragment(&builder.finish()))
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;

	use nml_document::Dependency;
	use nml_document::ElementKind;

	use crate::tests::compile;
	use crate::tests::compile_with;
	use crate::tests::html;

	fn body(text: &str) -> String {
		let compiled = compile(text);
		assert_diagnostics!(compiled.diagnostics, []);
		let html = html(&compiled);
		let start = html.find("<body>\n").unwrap() + 7;
		let end = html.find("</body>").unwrap();
		html[start..end].to_string()
	}

	#[test]
	fn roles_show_elements() {
		assert_eq!(
			body(":lua`nml.element('bold', {'a ', nml.element('italic', {'b'}), 1, 2.5})`"),
			"<p><strong>a <em>b</em>12.5</strong></p>\n"
		);
		assert_eq!(
			body(":lua`{'x', nml.element('code', {code = '<y>'})}`"),
			"<p>x<code>&lt;y&gt;</code></p>\n"
		);
		assert_eq!(
			body(
				":lua`nml.element('link', {url = 'https://x.y', id = 'l', classes = {'a', 'b'}, 'go'})`"
			),
			"<p><a id=\"l\" class=\"a b\" href=\"https://x.y\">go</a></p>\n"
		);
		// Strings are text, raw HTML is an element of its own.
		assert_eq!(
			body(":lua`'<b>'` :lua`nml.element('raw', {content = '<b>'})`"),
			"<p>&lt;b&gt; <b></p>\n"
		);
	}

	#[test]
	fn block_elements() {
		assert_eq!(
			body(
				":lua`nml.element('list', {numbered = true, start = 3, 'a', nml.element('list_item', {checkbox = 'checked', 'b'})})`"
			),
			"<ol start=\"3\">\n<li>a</li>\n<li><input type=\"checkbox\" disabled checked> b</li>\n</ol>\n"
		);
		assert_eq!(
			body(":lua`nml.element('callout', {type = 'tip', title = 'T', 'x'})`"),
			"<aside class=\"callout tip\">\n<p class=\"callout-title\">T</p>\nx</aside>\n"
		);
	}

	#[test]
	fn references() {
		let compiled = compile("# A {#a}\n:lua`nml.element('reference', {target = 'a'})`");
		assert_diagnostics!(compiled.diagnostics, []);
		assert_eq!(compiled.unit.uses.len(), 1);
		let html = html(&compiled);
		assert!(html.contains("<p><a href=\""), "{html}");
	}

	/// Where a file of the workspace is copied.
	fn copy_of(path: &str, content: &str) -> String {
		let hash = blake3::hash(content.as_bytes()).to_hex().to_string();
		nml_lang::asset_output_path(path, &hash)
	}

	#[test]
	fn media_paths_are_assets() {
		let compiled = compile_with(
			":lua`nml.element('media', {url = 'img/a.png', alt = 'A'})`\n\n:lua`nml.element('media', {url = 'https://x.y/b.png'})`\n",
			&[("img/a.png", "a")],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let page = html(&compiled);
		assert!(
			page.contains(&format!(
				"<img src=\"{}\" alt=\"A\">",
				copy_of("img/a.png", "a")
			)),
			"{page}"
		);
		assert!(page.contains("<img src=\"https://x.y/b.png\""), "{page}");
		assert_eq!(compiled.unit.assets.len(), 1);
		assert_eq!(compiled.unit.assets[0].path, "img/a.png");
		assert!(compiled.unit.dependencies.contains(&Dependency::File {
			path: "img/a.png".into(),
			hash: blake3::hash(b"a").to_hex().to_string(),
		}));

		// A missing file is reported at the script.
		let compiled = compile(":lua`nml.element('media', {url = 'no.png'})`\n");
		assert_diagnostics!(compiled.diagnostics, [3008 @ 0..44]);

		// After the parse, files are still added to the page.
		let compiled = compile_with(
			":lua`nml.post_parse(function() return nml.element('media', {url = 'img/a.png'}) end)`\n",
			&[("img/a.png", "a")],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert!(html(&compiled).contains(&copy_of("img/a.png", "a")));
		assert_eq!(compiled.unit.assets.len(), 1);
		let compiled = compile(
			":lua`nml.post_parse(function() return nml.element('media', {url = 'no.png'}) end)`\n",
		);
		assert_diagnostics!(compiled.diagnostics, [4001]);
	}

	#[test]
	fn figures() {
		let compiled = compile_with(
			":figure #a A\n![](a.png)\n\n:lua`nml.element('figure', {ref = 'b', caption = {'B ', nml.element('code', {code = 'x'})}, nml.element('media', {url = 'b.png'})})`\n\n:figure #c C\n![](c.png)\n\n:lua`nml.element('figure', {'Text.'})`\n",
			&[("a.png", "a"), ("b.png", "b"), ("c.png", "c")],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let html = html(&compiled);
		// Numbered in document order with the markup figures; the caption comes last.
		let figure = format!(
			"<figure id=\"b\">\n<img src=\"{}\" alt=\"\">\n<figcaption><span class=\"figure-number\">Figure 2:</span> B <code>x</code></figcaption>\n</figure>",
			copy_of("b.png", "b")
		);
		assert!(html.contains(&figure), "{html}");
		assert!(html.contains("Figure 3:</span> C"), "{html}");
		assert!(html.contains("<figure>\nText.\n</figure>"), "{html}");
		// Blocks stay blocks.
		let blocks = compile(
			":lua`nml.element('figure', {'a ', nml.element('bold', {'b'}), nml.element('code_block', {code = 'c'}), 'd'})`\n",
		);
		assert_diagnostics!(blocks.diagnostics, []);
		let document = &blocks.unit.document;
		let figure = document
			.descendants(document.root())
			.find(|(_, e)| matches!(e.kind, ElementKind::Figure { .. }))
			.unwrap()
			.0;
		let kinds: Vec<_> = document
			.children(figure)
			.map(|(_, e)| e.kind.name())
			.collect();
		assert_eq!(kinds, ["paragraph", "code_block", "paragraph"]);
		let exports: Vec<_> = compiled
			.unit
			.exports
			.iter()
			.map(|e| {
				(
					e.name.as_str(),
					e.property("title"),
					e.property("number"),
					e.property("media"),
				)
			})
			.collect();
		assert_eq!(
			exports,
			[
				("a", Some("A"), Some("1"), Some("a.png")),
				("b", Some("B x"), Some("2"), Some("b.png")),
				("c", Some("C"), Some("3"), Some("c.png")),
			]
		);
		// Declared at the script.
		assert_eq!(
			compiled.unit.exports[1].declared,
			Some(("page.nml".into(), 25..157))
		);

		// A figure's name is the unit's, whoever writes it.
		let compiled =
			compile(":figure #a A\nx\n\n:lua`nml.element('figure', {ref = 'a', 'y'})`\n");
		assert_diagnostics!(compiled.diagnostics, [2002 @ 16..61]);
	}

	#[test]
	fn figure_errors() {
		for (code, message) in [
			(
				"nml.element('figure', {ref = 'a b'})",
				"`a b` is not a reference name",
			),
			(
				"nml.element('figure', {ref = 'doc#a'})",
				"is not a reference name",
			),
			(
				"nml.element('figure', {ref = 'a', id = 'b'})",
				"cannot have an `id` too",
			),
			(
				"nml.post_parse(function() return nml.element('figure', {'x'}) end)",
				"a figure is numbered during the parse",
			),
		] {
			let compiled = compile(&format!(":lua`{code}`"));
			assert_diagnostics!(compiled.diagnostics, [4001]);
			let nml_diagnostics::DiagnosticKind::LuaError { message: error } =
				&compiled.diagnostics[0].kind
			else {
				unreachable!()
			};
			assert!(error.contains(message), "{code}: {error}");
		}
	}

	#[test]
	fn element_errors() {
		for (code, message) in [
			("nml.element('nope')", "no element kind is named `nope`"),
			("nml.element('heading', {level = 9})", "level"),
			("nml.element('link')", "needs the field `url`"),
			("nml.element('media', {url = 'a.txt'})", "set the `type`"),
			(
				"nml.element('layout', {type = 'centered', 'a', 'b'})",
				"1 part(s), not 2",
			),
			("nml.element('bold', {true})", "not a boolean"),
			(
				"nml.element('reference', {target = 'a b'})",
				"not a reference",
			),
		] {
			let compiled = compile(&format!(":lua`{code}`"));
			assert_diagnostics!(compiled.diagnostics, [4001]);
			let nml_diagnostics::DiagnosticKind::LuaError { message: error } =
				&compiled.diagnostics[0].kind
			else {
				unreachable!()
			};
			assert!(error.contains(message), "{code}: {error}");
		}
	}

	#[test]
	fn parse_markup() {
		// Parsed with the page's grammar and variables; strings stay text.
		let compiled = compile(
			":set name = *NML*\n:anchor a\n\n:lua`nml.parse('**bold** %name% [[a]]')` :lua`'**text**'` :lua``nml.element('italic', {nml.parse('`c`')})``",
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let html = html(&compiled);
		assert!(
			html.contains("<p><strong>bold</strong> <em>NML</em> <a href="),
			"{html}"
		);
		assert!(
			html.contains(" **text** <em><code>c</code></em></p>"),
			"{html}"
		);
		assert_eq!(compiled.unit.uses.len(), 1);
		assert_eq!(body(":lua`nml.parse('x').kind`"), "<p>markup</p>\n");
		// Errors in the markup are reported where the script is.
		let compiled = compile("Some :lua`nml.parse('**open')`");
		assert_diagnostics!(compiled.diagnostics, [1001]);
		let location = compiled.diagnostics[0].primary().location.root();
		assert_eq!(location.source().location().as_str(), "file:///page.nml");
		assert_eq!(location.range(), 5..5);
		// After the parse, it is an error.
		let compiled = compile(":lua`nml.post_parse(function() return nml.parse('**x**') end)`");
		assert_diagnostics!(compiled.diagnostics, [4001]);
	}

	#[test]
	fn anchors() {
		let compiled = compile(
			":lua`nml.element('anchor', {ref = 'a', title = 'The A'})` :lua`nml.element('anchor', {ref = 'b'})`\n\nSee [[a]].",
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let exports: Vec<_> = compiled
			.unit
			.exports
			.iter()
			.map(|export| (export.name.as_str(), export.property("title").unwrap()))
			.collect();
		assert_eq!(exports, [("a", "The A"), ("b", "b")]);
		let html = html(&compiled);
		assert!(html.contains("<span id=\"a\"></span>"), "{html}");
		// Declared twice, as `:anchor` would be.
		let compiled = compile(":anchor a\n:lua`nml.element('anchor', {ref = 'a'})`");
		assert_diagnostics!(compiled.diagnostics, [2002]);
	}

	#[test]
	fn rendered() {
		let compiled = compile(
			":lua`{nml.element('math', {source = 'x^2'}), nml.element('typst', {source = '$y$', block = true}), nml.element('tex', {source = 'T'}), nml.element('graph', {source = 'graph {}', layout = 'neato', width = '50%'})}`",
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		let kinds: Vec<_> = document
			.descendants(document.root())
			.filter_map(|(_, element)| match &element.kind {
				ElementKind::Rendered {
					kind,
					source,
					block,
				} => Some((kind.clone(), source.as_str(), *block)),
				_ => None,
			})
			.collect();
		assert_eq!(
			kinds,
			[
				(nml_document::RenderKind::Math, "x^2", false),
				(nml_document::RenderKind::Typst, "$y$", true),
				(
					nml_document::RenderKind::Tex {
						environment: "main".into()
					},
					"T",
					true
				),
				(
					nml_document::RenderKind::Graph {
						layout: "neato".into(),
						width: Some("50%".into())
					},
					"graph {}",
					true
				),
			]
		);
	}

	#[test]
	fn anchor_and_rendered_errors() {
		for (code, message) in [
			("nml.element('anchor')", "needs the field `ref`"),
			(
				"nml.element('anchor', {ref = 'a b'})",
				"is not a reference name",
			),
			("nml.element('math')", "needs the field `source`"),
			(
				"nml.element('graph', {source = 'x', layout = 'grid'})",
				"`grid` is not a Graphviz layout",
			),
			(
				"nml.post_parse(function() return nml.element('anchor', {ref = 'a'}) end)",
				"an anchor is declared during the parse",
			),
			(
				"nml.post_parse(function() return nml.element('math', {source = 'x'}) end)",
				"`math` is rendered from the parse",
			),
		] {
			let compiled = compile(&format!(":lua`{code}`"));
			assert_diagnostics!(compiled.diagnostics, [4001]);
			let nml_diagnostics::DiagnosticKind::LuaError { message: error } =
				&compiled.diagnostics[0].kind
			else {
				unreachable!()
			};
			assert!(error.contains(message), "{code}: {error}");
		}
	}

	#[test]
	fn code_in_a_language() {
		assert!(
			body(":lua`nml.element('code', {code = 'let x = 1;', language = 'rust'})`")
				.starts_with("<p><code class=\"highlighted\" data-language=\"rust\"><span")
		);
	}

	#[test]
	fn code_blocks() {
		assert!(
			body(":lua`nml.element('code_block', {code = 'x', max_lines = 3})`")
				.contains("<div class=\"code\" style=\"max-height: 3lh; overflow-y: auto\">")
		);
		let compiled = compile(":lua`nml.element('code_block', {code = 'x', max_lines = -1})`");
		assert_diagnostics!(compiled.diagnostics, [4001]);
	}

	#[test]
	fn tables() {
		// Rows are lists of cells; a cell is any value, or a `cell` for spans and alignment.
		assert_eq!(
			body(
				":lua`nml.element('table', {header = 1, align = {'left', 'right'}, {'Plan', 'Price'}, {nml.element('cell', {hspan = 2, align = 'center', 'Free'})}, {'Pro', {nml.element('italic', {'9'}), '.5'}}})`"
			),
			"<table>\n<thead>\n<tr><th style=\"text-align: left\">Plan</th><th style=\"text-align: right\">Price</th></tr>\n</thead>\n\
			<tbody>\n<tr><td colspan=\"2\" style=\"text-align: center\">Free</td></tr>\n\
			<tr><td style=\"text-align: left\">Pro</td><td style=\"text-align: right\"><em>9</em>.5</td></tr>\n</tbody>\n</table>\n"
		);
		assert_eq!(
			body(":lua`nml.element('table', {{nml.element('cell', {vspan = 2, 'a'}), 1}, {2.5}})`"),
			"<table>\n<tbody>\n<tr><td rowspan=\"2\">a</td><td>1</td></tr>\n<tr><td>2.5</td></tr>\n</tbody>\n</table>\n"
		);
		assert_eq!(
			body(":lua`nml.element('table', {}).kind`"),
			"<p>table</p>\n"
		);
		// Without a reference or a caption, a table can be made after the parse.
		let late =
			body(":lua`nml.post_parse(function() return nml.element('table', {{'x'}}) end)`");
		assert!(
			late.contains("<table>\n<tbody>\n<tr><td>x</td></tr>\n</tbody>\n</table>\n"),
			"{late}"
		);
	}

	#[test]
	fn numbered_tables() {
		let compiled = compile(
			":table #a A\n| x |\n\n:lua`nml.element('table', {ref = 'b', caption = {'B ', nml.element('code', {code = 'c'})}, {'y'}})`\n\n:lua`nml.element('table', {caption = 'C', {'z'}})`\n\n:table #d D\n| w |",
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let html = html(&compiled);
		// Numbered in document order with the markup tables; the caption comes first.
		assert!(
			html.contains(
				"<table id=\"b\">\n<caption><span class=\"table-number\">Table 2:</span> B <code>c</code></caption>\n<tbody>\n<tr><td>y</td></tr>"
			),
			"{html}"
		);
		assert!(html.contains("Table 3:</span> C</caption>"), "{html}");
		let exports: Vec<_> = compiled
			.unit
			.exports
			.iter()
			.map(|export| {
				(
					export.name.as_str(),
					export.property("number").unwrap(),
					export.property("title").unwrap(),
				)
			})
			.collect();
		assert_eq!(
			exports,
			[("a", "1", "A"), ("b", "2", "B c"), ("d", "4", "D")]
		);
	}

	#[test]
	fn table_errors() {
		for (code, message) in [
			(
				"nml.element('table', {header = 2, {'a'}})",
				"a table of 1 row(s) cannot have 2 header row(s)",
			),
			(
				"nml.element('table', {align = {'middle'}})",
				"`middle` is not an alignment",
			),
			(
				"nml.element('table', {'a'})",
				"a table's rows are lists of cells",
			),
			(
				"nml.element('cell', {hspan = 0})",
				"a cell's `hspan` is at least 1",
			),
			(
				"nml.element('cell', {vspan = -1})",
				"a cell's `vspan` is at least 1",
			),
			(
				"nml.element('cell', {align = 'top'})",
				"`top` is not an alignment",
			),
			(
				"nml.element('table', {ref = 'a', id = 'b'})",
				"a table's `ref` is its id",
			),
			(
				"nml.element('table', {ref = 'a b'})",
				"is not a reference name",
			),
			(
				"nml.post_parse(function() return nml.element('table', {caption = 'x'}) end)",
				"is numbered during the parse",
			),
		] {
			let compiled = compile(&format!(":lua`{code}`"));
			assert_diagnostics!(compiled.diagnostics, [4001]);
			let nml_diagnostics::DiagnosticKind::LuaError { message: error } =
				&compiled.diagnostics[0].kind
			else {
				unreachable!()
			};
			assert!(error.contains(message), "{code}: {error}");
		}
	}

	#[test]
	fn kinds() {
		assert_eq!(body(":lua`nml.element('bold').kind`"), "<p>style</p>\n");
	}
}
