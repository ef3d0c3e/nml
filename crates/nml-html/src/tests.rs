//! The HTML of NML elements, compiled from source text.

use std::sync::Arc;

use nml_lang::Language;
use nml_lang::MemoryResolver;
use nml_source::Source;
use nml_source::Url;

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
	let start = template.content.find("<body>\n").unwrap() + "<body>\n".len();
	let end = template.content.find("</body>").unwrap();
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
		"<h1 id=\"a\" class=\"unlisted x\">A</h1>\n"
	);
	assert_eq!(body("a :: b\n:: c\nd"), "<p>a \nd</p>\n");
}

#[test]
fn code_blocks() {
	assert_eq!(
		body("```[line_offset=9] A <title>\na < b\nc\n```"),
		"<figure class=\"code-block\">\n<figcaption>A &lt;title&gt;</figcaption>\n\
		<div class=\"code\"><pre class=\"line-numbers\" aria-hidden=\"true\">9\n10</pre>\
		<pre><code>a &lt; b\nc</code></pre></div>\n</figure>\n"
	);
	assert_eq!(
		body("```[numbers=false]\nx\n```"),
		"<figure class=\"code-block\">\n<div class=\"code\"><pre><code>x</code></pre></div>\n</figure>\n"
	);
	let html = body("```rust\nfn main() {}\n```");
	assert!(
		html.starts_with("<figure class=\"code-block\" data-language=\"rust\">"),
		"{html}"
	);
	assert!(html.contains("<span class=\"hl-"), "{html}");
}

#[test]
fn media() {
	assert_eq!(
		body_with(
			"![A \"flower\"](../img/a.png)[width=50%] ![Clip](c.webm) ![](https://x.y/s.mp3)",
			&[("img/a.png", ""), ("docs/c.webm", "")]
		),
		"<p><img src=\"../assets/img/a.png\" alt=\"A &quot;flower&quot;\" style=\"width: 50%\"> \
		<video src=\"../assets/docs/c.webm\" controls>Clip</video> \
		<audio src=\"https://x.y/s.mp3\" controls></audio></p>\n"
	);
}

#[test]
fn figures() {
	assert_eq!(
		body_with(":figure #f A *flower*\n![](a.png)", &[("docs/a.png", "")]),
		"<figure id=\"f\">\n<img src=\"../assets/docs/a.png\" alt=\"\">\n\
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
	assert_eq!(body("# A :raw`<sup>2</sup>`"), "<h1>A <sup>2</sup></h1>\n");
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

	let source = Source::new(Url::parse("file:///a.nml").unwrap(), "$x$".into(), 0);
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
	options.rendered.insert(id, "<svg/>".into());
	let page = compile(document, &options).content;
	assert!(page.contains("aria-label=\"x\"><svg/></span>"), "{page}");
}
