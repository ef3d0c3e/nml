//! The HTML of NML elements, compiled from source text.

use std::sync::Arc;

use nml_lang::Language;
use nml_lang::MemoryResolver;
use nml_source::Source;
use nml_source::Url;

use crate::DEFAULT_STYLE;
use crate::PageOptions;
use crate::compile;

/// The body of the page compiled from `text`, which must have no errors.
fn body(text: &str) -> String {
	body_with(text, &[])
}

/// The body of the page `docs/page.nml` compiled from `text`, with other files of the
/// workspace.
fn body_with(text: &str, files: &[(&str, &str)]) -> String {
	let source = Source::new(Url::parse("file:///docs/page.nml").unwrap(), text.into(), 0);
	let mut resolver = MemoryResolver::default();
	for (path, content) in files {
		resolver.files.insert(path.to_string(), content.to_string());
	}
	let compiled = nml_lang::compile(
		&Language::builtin(),
		Arc::new(source),
		"docs/page.nml",
		Arc::new(resolver),
	);
	assert!(
		compiled.diagnostics.is_empty(),
		"{:?}",
		compiled.diagnostics
	);
	let template = compile(&compiled.unit.document, &PageOptions::default());
	let start = template
		.content
		.find("<main class=\"nml-main\">\n")
		.unwrap()
		+ "<main class=\"nml-main\">\n".len();
	let end = template.content.find("</main>").unwrap();
	template.content[start..end].to_string()
}

#[test]
fn lists() {
	assert_eq!(
		body(" * a\n * *b*\n\ntext"),
		"<ul>\n<li>a</li>\n<li><em>b</em></li>\n</ul>\n<p>text</p>\n"
	);
	assert_eq!(body(" - a\n - b"), "<ol>\n<li>a</li>\n<li>b</li>\n</ol>\n");
	assert_eq!(
		body(" -[offset=3, bullet=a] a\n - b\n -[offset=9] c"),
		"<ol start=\"3\" type=\"a\">\n<li>a</li>\n<li>b</li>\n<li value=\"9\">c</li>\n</ol>\n"
	);
	assert_eq!(
		body(" *[bullet=square] a"),
		"<ul style=\"list-style-type: square\">\n<li>a</li>\n</ul>\n"
	);
	// Nested lists go in the item before them.
	assert_eq!(
		body(" * a\n *- b\n * c"),
		"<ul>\n<li>a<ol>\n<li>b</li>\n</ol>\n</li>\n<li>c</li>\n</ul>\n"
	);
}

#[test]
fn checkboxes() {
	assert_eq!(
		body(" * [ ] a\n * [-] b\n * [x] c"),
		"<ul>\n\
		<li><input type=\"checkbox\" disabled> a</li>\n\
		<li><input type=\"checkbox\" disabled aria-checked=\"mixed\"> b</li>\n\
		<li><input type=\"checkbox\" disabled checked> c</li>\n\
		</ul>\n"
	);
}

#[test]
fn inline_elements() {
	assert_eq!(
		body("__u__ ==h== ~~s~~ `a<b` [x **y**](a.html?q=1&r=\"2\")"),
		"<p><u>u</u> <mark>h</mark> <s>s</s> <code>a&lt;b</code> \
		<a href=\"a.html?q=1&amp;r=&quot;2&quot;\">x <strong>y</strong></a></p>\n"
	);
	assert_eq!(
		body("# A {#a .unlisted .x}"),
		"<h1 id=\"a\" class=\"unlisted x\"><a class=\"heading-link\" href=\"#a\">🔗</a> \
		<span class=\"heading-number\">1.</span> A</h1>\n"
	);
	assert_eq!(body("a :: b\n:: c\nd"), "<p>a \nd</p>\n");
}

#[test]
fn code_blocks() {
	assert_eq!(
		body("```[line_offset=9] A <title>\na < b\nc\n```"),
		"<figure class=\"code-block\">\n<figcaption>A &lt;title&gt;</figcaption>\n\
		<div class=\"code hl-code\"><pre class=\"line-numbers\" aria-hidden=\"true\">9\n10</pre>\
		<pre><code>a &lt; b\nc</code></pre></div>\n</figure>\n"
	);
	assert_eq!(
		body("```[numbers=false]\nx\n```"),
		"<figure class=\"code-block\">\n<div class=\"code hl-code\"><pre><code>x</code></pre></div>\n</figure>\n"
	);
	let html = body("```rust\nfn main() {}\n```");
	assert!(
		html.starts_with("<figure class=\"code-block\" data-language=\"rust\">"),
		"{html}"
	);
	assert!(html.contains("<span class=\"hl-"), "{html}");
	// Inline code in a language is highlighted.
	let html = body("a :code[rust]`let x = 1;` b");
	assert!(
		html.starts_with(
			"<p>a <code class=\"highlighted hl-code\" data-language=\"rust\"><span class=\"hl-"
		),
		"{html}"
	);
	assert!(html.ends_with("</code> b</p>\n"), "{html}");
	// The numbers and the code scroll together past `max_lines`.
	assert_eq!(
		body("```[max_lines=2]\na\nb\nc\n```"),
		"<figure class=\"code-block\">\n\
		<div class=\"code hl-code\" style=\"max-height: 2lh; overflow-y: auto\">\
		<pre class=\"line-numbers\" aria-hidden=\"true\">1\n2\n3</pre>\
		<pre><code>a\nb\nc</code></pre></div>\n</figure>\n"
	);
}

#[test]
fn media() {
	assert_eq!(
		body_with(
			"![A \"flower\"](../img/a.png)[width=50%] ![Clip](c.webm) ![](https://x.y/s.mp3)",
			&[("img/a.png", ""), ("docs/c.webm", "")]
		),
		"<p><img src=\"../assets/af1349b9f5f9a1a6.png\" alt=\"A &quot;flower&quot;\" style=\"width: 50%\"> \
		<video src=\"../assets/af1349b9f5f9a1a6.webm\" controls>Clip</video> \
		<audio src=\"https://x.y/s.mp3\" controls></audio></p>\n"
	);
}

#[test]
fn figures() {
	assert_eq!(
		body_with(":figure #f A *flower*\n![](a.png)", &[("docs/a.png", "")]),
		"<figure id=\"f\">\n<img src=\"../assets/af1349b9f5f9a1a6.png\" alt=\"\">\n\
		<figcaption><span class=\"figure-number\">Figure 1:</span> A <em>flower</em></figcaption>\n\
		</figure>\n"
	);
	assert_eq!(
		body(":figure\n * a"),
		"<figure>\n<ul>\n<li>a</li>\n</ul>\n</figure>\n"
	);
}

#[test]
fn raw_output() {
	assert_eq!(
		body("a :raw`<br>` b\n:raw\n```\n<hr class=\"x\">\n```"),
		"<p>a <br> b</p>\n<hr class=\"x\">\n"
	);
	assert!(body("# A :raw`<sup>2</sup>`").ends_with("A <sup>2</sup></h1>\n"));
}

#[test]
fn headings() {
	/// Each heading as `id number text`.
	fn headings(text: &str) -> Vec<String> {
		body(text)
			.lines()
			.filter(|line| line.starts_with("<h"))
			.map(|line| {
				let id = line.split('"').nth(1).unwrap();
				let number = line
					.split("<span class=\"heading-number\">")
					.nth(1)
					.map_or("-", |rest| rest.split('<').next().unwrap());
				let text = line.split_once("</a> ").unwrap().1;
				let text = text.split_once("</span> ").map_or(text, |(_, text)| text);
				let text = text.split("</h").next().unwrap();
				format!("{id} {number} {text}")
			})
			.collect()
	}
	// Levels open at 1, deeper ones close, and unnumbered headings do not count.
	assert_eq!(
		headings("# Intro\n### Deep\n## Two\n## Off {.unnumbered}\n## Three\n# Next\n## Again"),
		[
			"intro 1. Intro",
			"deep 1.1.1. Deep",
			"two 1.2. Two",
			"off - Off",
			"three 1.3. Three",
			"next 2. Next",
			"again 2.1. Again"
		]
	);
	// Ids come from the text, are unique, and never take an id of the page.
	assert_eq!(
		headings("# Café, *au* lait!\n# Same\n# Same\n# Taken\n:anchor taken\n# ?"),
		[
			"café-au-lait 1. Café, <em>au</em> lait!",
			"same 2. Same",
			"same-2 3. Same",
			"taken-2 4. Taken",
			"section 5. ?"
		]
	);
	assert_eq!(
		body("## B {#b}"),
		"<h2 id=\"b\"><a class=\"heading-link\" href=\"#b\">🔗</a> \
		<span class=\"heading-number\">1.1.</span> B</h2>\n"
	);
}

#[test]
fn quotes() {
	assert_eq!(
		body("> a\n>\n> b"),
		"<blockquote>\n<p>a</p>\n<p>b</p>\n</blockquote>\n"
	);
	assert_eq!(
		body("> [!quote][author=Ada, cite=<Notes>, url=\"https://x.y\"]\n> x"),
		"<figure class=\"quote\">\n<blockquote cite=\"https://x.y\">\n<p>x</p>\n</blockquote>\n\
		<figcaption>— Ada, <cite>&lt;Notes&gt;</cite></figcaption>\n</figure>\n"
	);
	assert_eq!(
		body("> [!quote][cite=Notes]\n> x"),
		"<figure class=\"quote\">\n<blockquote>\n<p>x</p>\n</blockquote>\n\
		<figcaption><cite>Notes</cite></figcaption>\n</figure>\n"
	);
}

#[test]
fn callouts() {
	assert_eq!(
		body("> [!warning]\n> x"),
		"<aside class=\"callout warning\">\n<p class=\"callout-title\">Warning</p>\n<p>x</p>\n</aside>\n"
	);
	assert_eq!(
		body("> [!TIP] A *tip*\n> x"),
		"<aside class=\"callout tip\">\n<p class=\"callout-title\">A <em>tip</em></p>\n<p>x</p>\n</aside>\n"
	);
}

#[test]
fn layouts() {
	assert_eq!(
		body(":layout split\na\n:next\nb\n:end"),
		"<div class=\"layout split\" style=\"display: flex; gap: 1em\">\n\
		<div class=\"part\" style=\"flex: 1\">\n<p>a</p>\n</div>\n\
		<div class=\"part\" style=\"flex: 1\">\n<p>b</p>\n</div>\n</div>\n"
	);
	assert_eq!(
		body(":layout centered\na\n:end"),
		"<div class=\"layout centered\" style=\"text-align: center\">\n\
		<div class=\"part\">\n<p>a</p>\n</div>\n</div>\n"
	);
	assert_eq!(
		body(":layout spoiler The *answer*\n42\n:end"),
		"<details class=\"layout spoiler\"><summary>The <em>answer</em></summary>\n\
		<div class=\"part\">\n<p>42</p>\n</div>\n</details>\n"
	);
	assert!(body(":layout spoiler\n42\n:end").contains("<summary>Spoiler</summary>"));
}

#[test]
fn tables() {
	assert_eq!(
		body(":table #t Prices\n|Plan|Price|\n|:--|--:|\n|[hspan=2]Free|\n|Pro|*9*|"),
		"<table id=\"t\">\n<caption><span class=\"table-number\">Table 1:</span> Prices</caption>\n\
		<thead>\n<tr><th style=\"text-align: left\">Plan</th><th style=\"text-align: right\">Price</th></tr>\n</thead>\n\
		<tbody>\n<tr><td colspan=\"2\" style=\"text-align: left\">Free</td></tr>\n\
		<tr><td style=\"text-align: left\">Pro</td><td style=\"text-align: right\"><em>9</em></td></tr>\n</tbody>\n\
		</table>\n"
	);
	assert_eq!(
		body("|[vspan=2]a|b|\n|c|"),
		"<table>\n<tbody>\n<tr><td rowspan=\"2\">a</td><td>b</td></tr>\n<tr><td>c</td></tr>\n</tbody>\n</table>\n"
	);
	// A cell's own alignment beats its column's; a cell under a row span is in the column
	// after it.
	assert_eq!(
		body("|a|b|c|\n|:-|:-:|-:|\n|[vspan=2]d|[align=right]e|f|\n|g|h|"),
		"<table>\n<thead>\n<tr><th style=\"text-align: left\">a</th><th style=\"text-align: center\">b</th>\
		<th style=\"text-align: right\">c</th></tr>\n</thead>\n<tbody>\n\
		<tr><td rowspan=\"2\" style=\"text-align: left\">d</td><td style=\"text-align: right\">e</td>\
		<td style=\"text-align: right\">f</td></tr>\n\
		<tr><td style=\"text-align: center\">g</td><td style=\"text-align: right\">h</td></tr>\n</tbody>\n</table>\n"
	);
}

#[test]
fn anchors() {
	assert_eq!(
		body("a\n:anchor x\nb"),
		"<p>a</p>\n<span id=\"x\"></span>\n<p>b</p>\n"
	);
}

#[test]
fn rendered() {
	assert_eq!(
		body("a $x<1$ b\n\n:graph\n```\ngraph {}\n```"),
		"<p>a <span class=\"rendered math\" role=\"img\" aria-label=\"x&lt;1\"><code>x&lt;1</code></span> b</p>\n\
		<div class=\"rendered graph\" role=\"img\" aria-label=\"graph {}\"><code>graph {}</code></div>\n"
	);

	// With the rendered image.
	let page = |text: &str| {
		let source = Source::new(Url::parse("file:///a.nml").unwrap(), text.into(), 0);
		let compiled = nml_lang::compile(
			&Language::builtin(),
			Arc::new(source),
			"a.nml",
			Arc::new(MemoryResolver::default()),
		);
		let document = &compiled.unit.document;
		let (id, _) = document
			.descendants(document.root())
			.find(|(_, element)| matches!(element.kind, nml_document::ElementKind::Rendered { .. }))
			.unwrap();
		let mut options = PageOptions::default();
		options.rendered.insert(id, "<svg width=\"62pt\"/>".into());
		compile(document, &options).content
	};
	let math = page("$x$");
	assert!(
		math.contains("aria-label=\"x\"><svg width=\"62pt\"/></span>"),
		"{math}"
	);
	// A graph with a width fills a box of that width.
	let graph = page(":graph[width=50%]\n```\ngraph {}\n```");
	assert!(
		graph.contains(
			"<div class=\"rendered graph\" role=\"img\" aria-label=\"graph {}\" style=\"width: 50%\">\
			<svg style=\"width: 100%; height: auto\" width=\"62pt\"/></div>"
		),
		"{graph}"
	);
}

/// The classes the writer gives elements, from its source: `class=\"a b\"` and the
/// classes passed to `open_with`. Those made at run time, `callout {kind}`, are left out.
fn written_classes() -> Vec<String> {
	let source = include_str!("template.rs");
	let source = &source[..source.find("#[cfg(test)]").unwrap()];
	let mut found = Vec::new();
	let mut rest = source;
	while let Some(start) = rest.find("class=\\\"") {
		rest = &rest[start + 8..];
		found.push(&rest[..rest.find("\\\"").unwrap()]);
	}
	for call in source.split("open_with(").skip(1) {
		let arguments = &call[..call.find(");").unwrap()];
		if let Some(start) = arguments.find("Some(\"") {
			let class = &arguments[start + 6..];
			found.push(&class[..class.find('"').unwrap()]);
		}
	}
	let mut classes = Vec::new();
	for class in found.iter().flat_map(|classes| classes.split(' ')) {
		let identifier =
			!class.is_empty() && (class.chars()).all(|c| c.is_ascii_alphanumeric() || c == '-');
		if identifier {
			classes.push(class.to_string());
		}
	}
	classes.sort();
	classes.dedup();
	classes
}

/// Whether the stylesheet has a selector with `.class`.
fn styles(css: &str, class: &str) -> bool {
	let selector = format!(".{class}");
	css.match_indices(&selector).any(|(at, _)| {
		let next = css[at + selector.len()..].chars().next();
		!next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
	})
}

#[test]
fn the_default_style_covers_the_output() {
	let css = DEFAULT_STYLE;
	let written = written_classes();
	// The scan finds them.
	for class in [
		"nml-page",
		"nml-main",
		"heading-link",
		"code-block",
		"media-number",
	] {
		assert!(written.iter().any(|c| c == class), "{class} in {written:?}");
	}
	// Styled by the code theme in `nml-code.css`, or by the element they are on.
	let elsewhere = ["hl-code", "hl-", "line-numbers", "highlighted"];
	for class in &written {
		if !elsewhere.contains(&class.as_str()) {
			assert!(styles(css, class), "`.{class}` is not styled");
		}
	}
	for kind in nml_document::CalloutKind::ALL {
		assert!(styles(css, kind.name()), "callout `{}`", kind.name());
	}
	for class in [
		"layout",
		"rendered",
		"graph",
		"nml-unresolved",
		"nml-sidebar",
	] {
		assert!(styles(css, class), "`.{class}` is not styled");
	}
	// `std.toc` and `std.nav`.
	for class in [
		"toc",
		"toc-title",
		"unnumbered",
		"nav",
		"current",
		"category",
	] {
		assert!(styles(css, class), "`.{class}` is not styled");
	}
}

/// The custom properties set in the block that `block` starts with.
fn properties(block: &str) -> Vec<(&str, &str)> {
	let block = &block[block.find('{').unwrap() + 1..];
	let block = &block[..block.find('}').unwrap()];
	block
		.split(';')
		.filter_map(|declaration| {
			let (name, value) = declaration.split_once(':')?;
			let name = name.trim();
			name.starts_with("--").then(|| (name, value.trim()))
		})
		.collect()
}

#[test]
fn the_default_style_has_both_color_schemes() {
	let css = DEFAULT_STYLE;
	let mut depth = 0i32;
	for c in css.chars() {
		depth += match c {
			'{' => 1,
			'}' => -1,
			_ => 0,
		};
		assert!(depth >= 0, "unbalanced braces");
	}
	assert_eq!(depth, 0, "unbalanced braces");

	let light = properties(&css[css.find(":root {").unwrap()..]);
	let dark_media = css.find("@media (prefers-color-scheme: dark) {").unwrap();
	let dark_root = dark_media + css[dark_media..].find(":root {").unwrap();
	let dark = properties(&css[dark_root..]);
	let colors: Vec<_> = (light.iter())
		.filter(|(_, value)| value.starts_with('#'))
		.map(|(name, _)| *name)
		.collect();
	assert!(colors.len() >= 10, "{colors:?}");
	let dark_names: Vec<_> = dark.iter().map(|(name, _)| *name).collect();
	// Every color is set for the dark scheme, to something else, and nothing more is.
	assert_eq!(colors, dark_names);
	for (name, value) in &dark {
		let light = light.iter().find(|(n, _)| n == name).unwrap().1;
		assert_ne!(*value, light, "{name}");
	}

	// Every property used is set.
	let mut rest = css;
	while let Some(at) = rest.find("var(") {
		rest = &rest[at + 4..];
		let name = &rest[..rest.find(')').unwrap()];
		let name = name.split(',').next().unwrap().trim();
		let set = light.iter().any(|(n, _)| *n == name) || css.contains(&format!("{name}:"));
		assert!(set, "`{name}` is used but never set");
	}
}

#[test]
fn highlighted_code_has_the_theme_colors() {
	// `nml-code.css` gives `.hl-code` the theme's text and background.
	let html = body("```rust\nfn main() {}\n```\n\n:code[rust]`x`");
	assert!(html.contains("<div class=\"code hl-code\">"), "{html}");
	assert!(
		html.contains("<code class=\"highlighted hl-code\""),
		"{html}"
	);
	let css = nml_render::stylesheet(nml_render::DEFAULT_THEME, nml_render::DEFAULT_DARK_THEME);
	assert!(css.unwrap().contains(".hl-code {"));
}
