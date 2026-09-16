//! Code highlighting with syntect. Highlighted code carries CSS classes, and a theme is a
//! stylesheet: changing it does not change the pages.

use std::sync::OnceLock;

use syntect::highlighting::ThemeSet;
use syntect::html::ClassStyle;
use syntect::html::ClassedHTMLGenerator;
use syntect::html::css_for_theme_with_class_style;
use syntect::parsing::SyntaxReference;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// The prefix of the highlighting classes, so that they do not clash with a page's own.
const CLASS_STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "hl-" };

/// The theme of the light color scheme when the configuration names none.
pub const DEFAULT_THEME: &str = "InspiredGitHub";
/// The theme of the dark color scheme when the configuration names none.
pub const DEFAULT_DARK_THEME: &str = "base16-ocean.dark";

fn syntaxes() -> &'static SyntaxSet {
	static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
	SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn themes() -> &'static ThemeSet {
	static THEMES: OnceLock<ThemeSet> = OnceLock::new();
	THEMES.get_or_init(ThemeSet::load_defaults)
}

fn syntax(language: &str) -> Option<&'static SyntaxReference> {
	let syntaxes = syntaxes();
	syntaxes
		.find_syntax_by_token(language)
		.or_else(|| syntaxes.find_syntax_by_name(language))
}

/// Whether code in `language` (a name, like `Rust`, or a file extension, like `rs`) can be
/// highlighted.
pub fn is_known_language(language: &str) -> bool {
	syntax(language).is_some()
}

/// The languages code can be highlighted in: their name and file extensions.
pub fn languages() -> Vec<(&'static str, &'static [String])> {
	syntaxes()
		.syntaxes()
		.iter()
		.map(|syntax| (syntax.name.as_str(), syntax.file_extensions.as_slice()))
		.collect()
}

/// The names of the built-in themes.
pub fn theme_names() -> Vec<&'static str> {
	themes().themes.keys().map(String::as_str).collect()
}

/// The HTML of highlighted code, without the `<pre>` around it. Code in an unknown language,
/// or none, is only escaped.
pub fn highlight(code: &str, language: Option<&str>) -> String {
	let Some(syntax) = language.and_then(syntax) else {
		return escape(code);
	};
	let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, syntaxes(), CLASS_STYLE);
	for line in LinesWithEndings::from(code) {
		if generator
			.parse_html_for_line_which_includes_newline(line)
			.is_err()
		{
			// The syntax definition failed on this code: show it plain.
			return escape(code);
		}
	}
	generator.finalize()
}

/// The stylesheet of highlighted code: the layout of code blocks, the colors of `light`,
/// and those of `dark` for the dark color scheme. `None` if a theme has no such name.
pub fn stylesheet(light: &str, dark: &str) -> Option<String> {
	let colors = |theme: &str| {
		let theme = themes().themes.get(theme)?;
		css_for_theme_with_class_style(theme, CLASS_STYLE).ok()
	};
	let (light, dark) = (colors(light)?, colors(dark)?);
	Some(format!(
		"{LAYOUT}\n{light}\n@media (prefers-color-scheme: dark) {{\n{dark}}}\n"
	))
}

/// The layout of code blocks: line numbers in a gutter that is not selected with the code.
const LAYOUT: &str = "\
.code-block .code { display: flex; overflow-x: auto; }
.code-block pre { margin: 0; }
.code-block .line-numbers { user-select: none; text-align: right; padding-right: 1em; opacity: 0.6; }
";

fn escape(text: &str) -> String {
	let mut out = String::with_capacity(text.len());
	for c in text.chars() {
		match c {
			'&' => out.push_str("&amp;"),
			'<' => out.push_str("&lt;"),
			'>' => out.push_str("&gt;"),
			c => out.push(c),
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn highlights_known_languages() {
		let html = highlight("fn main() {}\n", Some("rust"));
		assert!(html.contains("<span class=\"hl-"), "{html}");
		assert!(html.contains("main"), "{html}");
		// By name and by extension.
		assert_eq!(
			highlight("fn a() {}", Some("Rust")),
			highlight("fn a() {}", Some("rs"))
		);
		assert!(is_known_language("rs"));
		assert!(!is_known_language("nope"));
	}

	#[test]
	fn other_code_is_escaped() {
		assert_eq!(highlight("a < b && c", None), "a &lt; b &amp;&amp; c");
		assert_eq!(highlight("<x>", Some("nope")), "&lt;x&gt;");
	}

	#[test]
	fn code_is_escaped_when_highlighted() {
		let html = highlight("let s = \"<b>\";\n", Some("rust"));
		assert!(!html.contains("<b>"), "{html}");
		assert!(html.contains("&lt;b&gt;"), "{html}");
	}

	#[test]
	fn stylesheets() {
		let css = stylesheet(DEFAULT_THEME, DEFAULT_DARK_THEME).unwrap();
		assert!(css.contains(".hl-"), "{css}");
		assert!(css.contains(".line-numbers"));
		// The dark colors come after the light ones, for the dark scheme only.
		let (light, dark) = css
			.split_once("@media (prefers-color-scheme: dark) {")
			.unwrap();
		assert!(light.contains(".hl-"), "{css}");
		assert!(dark.contains(".hl-"), "{css}");
		assert!(dark.trim_end().ends_with('}'));
		assert_eq!(
			css.matches('{').count(),
			css.matches('}').count(),
			"balanced: {css}"
		);
		assert_ne!(light, dark);
		assert!(stylesheet("nope", DEFAULT_DARK_THEME).is_none());
		assert!(stylesheet(DEFAULT_THEME, "nope").is_none());
		assert!(theme_names().contains(&DEFAULT_THEME));
		assert!(theme_names().contains(&DEFAULT_DARK_THEME));
	}
}
