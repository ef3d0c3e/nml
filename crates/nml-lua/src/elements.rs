//! Elements built by scripts with `nml.element`, and the values scripts show: strings and
//! numbers are text, elements are markup, and lists hold several of them.

use std::sync::Arc;

use mlua::AnyUserData;
use mlua::Lua;
use mlua::Table;
use mlua::UserData;
use mlua::UserDataFields;
use mlua::Value;
use nml_document::Attributes;
use nml_document::BulletStyle;
use nml_document::CalloutKind;
use nml_document::Checkbox;
use nml_document::DocumentBuilder;
use nml_document::ElementKind;
use nml_document::LayoutKind;
use nml_document::ListKind;
use nml_document::MediaKind;
use nml_document::NumberStyle;
use nml_document::Style;
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
}

impl UserData for Element {
	fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
		fields.add_field_method_get("kind", |_, element| {
			Ok(match &element.content {
				Content::Element(kind) => kind.name(),
				Content::Reference(_) => "reference",
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
	("code", "code"),
	("code_block", "code, language, line_offset, numbers"),
	("media", "url, type, alt, width"),
	("list", "numbered, start, bullet"),
	("list_item", "checkbox, value"),
	("quote", "author, cite, url"),
	("callout", "type, title"),
	("layout", "type, title"),
	("raw", "content, block"),
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
/// | `code` | `code` |
/// | `code_block` | `code`, `language`, `line_offset`, `numbers`; children are the title |
/// | `media` | `url`, `type` (`image`, `video`, `audio`), `alt`, `width` |
/// | `list` | `numbered`, `start`, `bullet` (`1`, `a`, `A`, `i`, `I` if numbered; `disc`, `circle`, `square`, `none` if not); children that are not items become items |
/// | `list_item` | `checkbox` (`unchecked`, `partial`, `checked`), `value` (its number) |
/// | `quote` | `author`, `cite`, `url` |
/// | `callout` | `type` (`note`, `tip`, …), `title` |
/// | `layout` | `type` (`centered`, `split`, `spoiler`), `title`; each child is a part |
/// | `raw` | `content` (HTML, written as is), `block` |
///
/// Every kind takes `id` and `classes`. References only work where the element goes into
/// the document as it is parsed (roles and rules), not in post-parse or deferred output.
#[api(returns = "nml.Node")]
fn element(lua: &Lua, kind: String, spec: Option<Table>) -> mlua::Result<AnyUserData> {
	let spec = match spec {
		Some(spec) => spec,
		None => lua.create_table()?,
	};
	lua.create_userdata(build(&kind, &spec)?)
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
		"code" => Content::Element(ElementKind::Code(required("code")?)),
		"code_block" => {
			let numbered = spec.get::<Option<bool>>("numbers")?.unwrap_or(true);
			let first = spec.get::<Option<i64>>("line_offset")?.unwrap_or(1);
			Content::Element(ElementKind::CodeBlock {
				language: text("language")?,
				code: required("code")?,
				first_line: numbered.then_some(first),
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
	let attributes = Attributes {
		id: text("id")?,
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
		open(lower, element, token);
		lower.builder.close();
	}
}

/// Opens an element in the document being lowered, and writes its children. The caller
/// closes it.
pub(crate) fn open(lower: &mut Lower<'_, '_>, element: &Element, token: &Token) {
	match &element.content {
		Content::Element(kind) => {
			lower
				.builder
				.open_with(kind.clone(), token.clone(), element.attributes.clone());
			self::lower(lower, &element.children, token);
		}
		Content::Reference(target) => {
			lower.open_reference(target.clone(), token.clone(), token.clone());
			if element.children.is_empty() {
				lower.reference_title(target.clone(), token.clone());
			} else {
				self::lower(lower, &element.children, token);
			}
		}
	}
}

/// The HTML of elements, for output made after the parse (post-parse and deferred). They
/// cannot hold references, which need the link stage.
pub(crate) fn html(elements: &[Element]) -> mlua::Result<String> {
	let source = Arc::new(Source::new(
		Url::parse("virtual:///script-output").unwrap(),
		String::new(),
		0,
	));
	let token = Token::new(source.clone(), 0..0);
	let mut builder = DocumentBuilder::new(source);
	fn write(
		builder: &mut DocumentBuilder<'_>,
		elements: &[Element],
		token: &Token,
	) -> mlua::Result<()> {
		for element in elements {
			let Content::Element(kind) = &element.content else {
				return Err(mlua::Error::runtime(
					"a reference needs the parse: it cannot be made after it",
				));
			};
			builder.open_with(kind.clone(), token.clone(), element.attributes.clone());
			write(builder, &element.children, token)?;
			builder.close();
		}
		Ok(())
	}
	write(&mut builder, elements, &token)?;
	Ok(nml_html::fragment(&builder.finish()))
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;

	use crate::tests::compile;
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
	fn kinds() {
		assert_eq!(body(":lua`nml.element('bold').kind`"), "<p>style</p>\n");
	}
}
