use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::DiagnosticKind;
use nml_diagnostics::DiagnosticMessage;
use nml_document::Asset;
use nml_document::Attributes;
use nml_document::Dependency;
use nml_document::ElementId;
use nml_document::ElementKind;
use nml_document::Export;
use nml_document::MediaKind;
use nml_document::SymbolKind;
use nml_grammar::Atom;
use nml_grammar::OptionKind;
use nml_grammar::OptionSchema;
use nml_grammar::OptionSpec;
use nml_grammar::OptionValue;
use nml_grammar::Rule;
use nml_grammar::RuleError;
use nml_grammar::Value;

use crate::Language;
use crate::Lower;
use crate::ResolveError;
use crate::language::plain_text;
use crate::resolve::asset_output_path;
use crate::resolve::relative_output_url;

/// Media `![alt](path)[options]` and figures `:figure #ref Caption`, which apply to the
/// next block.
pub(crate) fn add(language: &mut Language) -> Result<(), RuleError> {
	let schema = OptionSchema::new([
		OptionSpec::new(
			"type",
			OptionKind::Choice(
				["auto", "image", "video", "audio"]
					.map(String::from)
					.to_vec(),
			),
		)
		.with_default(OptionValue::Text("auto".into()))
		.with_doc("The kind of medium; `auto` tells it from the file's extension."),
		OptionSpec::new("width", OptionKind::Text)
			.with_default(OptionValue::Text(String::new()))
			.with_doc("A CSS width, such as `50%` or `20em`."),
	]);
	let media = Rule::new(
		"media",
		vec![
			Atom::capture("open", Atom::literal("![")).with_class("operator"),
			Atom::capture("alt", Atom::InlineContent),
			Atom::capture("middle", Atom::literal("](")).with_class("operator"),
			Atom::capture("url", Atom::Text).with_class("string"),
			Atom::capture("close", Atom::literal(")")).with_class("operator"),
			Atom::optional(vec![Atom::capture(
				"options",
				Atom::Options(Arc::new(schema)),
			)]),
		],
	);
	language.add(media, lower_media)?;

	let figure = Rule::new(
		"figure",
		vec![
			Atom::LineStart,
			Atom::capture("command", Atom::literal(":figure")).with_class("keyword"),
			Atom::optional(vec![
				Atom::literal(" #"),
				Atom::capture("ref", Atom::Word).with_class("variable"),
			]),
			Atom::optional(vec![
				Atom::literal(" "),
				Atom::capture("caption", Atom::InlineContent),
			]),
			Atom::LineEnd,
			Atom::capture("body", Atom::Block),
		],
	);
	language.add(figure, lower_figure)?;
	Ok(())
}

/// The URL of a medium written `written` in the file being lowered. An external URL is
/// kept; a path is resolved in the workspace, its file recorded as an asset of the page and a
/// dependency, and the URL goes from the page to the asset's copy.
pub fn media_url(lower: &mut Lower<'_, '_>, written: &str) -> Result<String, ResolveError> {
	if is_external(written) {
		return Ok(written.to_string());
	}
	let from = lower.files.last().unwrap().clone();
	let (path, hash) = lower.resolver.asset(&from, written)?;
	let asset = Asset {
		output: asset_output_path(&path, &hash),
		path: path.clone(),
	};
	let url = relative_output_url(&lower.output_path(), &asset.output);
	lower.depend(Dependency::File { path, hash });
	if !lower.assets.contains(&asset) {
		lower.assets.push(asset);
	}
	Ok(url)
}

/// Whether a URL leaves the workspace: it has a scheme (`https:`, `data:`) or a host.
pub fn is_external(url: &str) -> bool {
	url.starts_with("//")
		|| url.split_once(':').is_some_and(|(scheme, _)| {
			scheme
				.chars()
				.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
				&& scheme.len() > 1
		})
}

fn lower_media(lower: &mut Lower<'_, '_>, m: &nml_grammar::RuleMatch) {
	let url_token = m.capture("url").unwrap().token.clone();
	let written = url_token.content().trim();
	let options = match m.capture("options").map(|capture| &capture.value) {
		Some(Value::Options(options)) => Some(options),
		_ => None,
	};
	let option = |name| match options.and_then(|options| options.get(name)) {
		Some(OptionValue::Text(text)) if !text.is_empty() => Some(text.clone()),
		_ => None,
	};
	let kind = match option("type").as_deref() {
		Some("image") => Some(MediaKind::Image),
		Some("video") => Some(MediaKind::Video),
		Some("audio") => Some(MediaKind::Audio),
		_ => MediaKind::from_path(written),
	};
	let Some(kind) = kind else {
		let kind = DiagnosticKind::UnknownMediaType {
			url: written.into(),
		};
		let message = DiagnosticMessage::new(url_token);
		lower.diagnostics.push(Diagnostic::new(kind, message));
		return;
	};

	let url = match media_url(lower, written) {
		Ok(url) => url,
		Err(_) => {
			let kind = DiagnosticKind::FileNotFound {
				path: written.into(),
			};
			let message = DiagnosticMessage::new(url_token);
			lower.diagnostics.push(Diagnostic::new(kind, message));
			return;
		}
	};
	let alt = match m.capture("alt").map(|capture| &capture.value) {
		Some(Value::Nodes(nodes)) => plain_text(nodes),
		_ => String::new(),
	};
	let media = ElementKind::Media {
		kind,
		url,
		alt,
		width: option("width"),
	};
	lower.builder.leaf(media, m.token.clone());
}

/// The number of the last figure of a unit.
#[derive(Default)]
struct Figures(u32);

/// Takes the next number of the unit's figures: they are numbered in the order they are
/// written, by markup or by scripts.
pub fn next_figure(lower: &mut Lower<'_, '_>) -> u32 {
	let figures = lower.extension(Figures::default);
	figures.0 += 1;
	figures.0
}

/// The symbol of a figure: its caption's text is its title, and `medium` (the first asset of
/// its body) is shown by editors.
pub fn figure_export(
	name: &str,
	element: ElementId,
	number: u32,
	title: String,
	medium: Option<String>,
) -> Export {
	let export = Export::new(name, SymbolKind::Figure, Some(element))
		.with("title", title)
		.with("number", number.to_string());
	match medium {
		Some(medium) => export.with("media", medium),
		None => export,
	}
}

fn lower_figure(lower: &mut Lower<'_, '_>, m: &nml_grammar::RuleMatch) {
	let number = next_figure(lower);
	let reference = m.capture("ref").map(|capture| capture.token.clone());
	let attributes = Attributes {
		id: reference.as_ref().map(|r| r.content().to_string()),
		classes: Vec::new(),
	};
	lower
		.builder
		.open_with(ElementKind::Figure { number }, m.token.clone(), attributes);
	let assets = lower.assets.len();
	lower.capture(m, "body");
	let medium = lower.assets.get(assets).map(|asset| asset.path.clone());
	let caption = m.capture("caption");
	if let Some(caption) = caption {
		lower
			.builder
			.open(ElementKind::Caption, caption.token.clone());
		lower.capture(m, "caption");
		lower.builder.close();
	}
	let element = lower.builder.close();

	if let Some(reference) = reference {
		let title = match caption.map(|capture| &capture.value) {
			Some(Value::Nodes(nodes)) => plain_text(nodes),
			_ => String::new(),
		};
		let export = figure_export(reference.content(), element, number, title, medium);
		lower.export(export, reference);
	}
}

#[cfg(test)]
mod tests {
	use nml_diagnostics::assert_diagnostics;
	use nml_document::Asset;
	use nml_document::Dependency;
	use nml_document::ElementKind;
	use nml_document::MediaKind;

	use crate::resolve::asset_output_path;
	use crate::tests::compile_with;

	fn media(text: &str, files: &[(&str, &str)]) -> Vec<ElementKind> {
		let compiled = compile_with(text, files);
		assert_diagnostics!(compiled.diagnostics, []);
		media_of(&compiled)
	}

	fn media_of(compiled: &crate::Compiled) -> Vec<ElementKind> {
		let document = &compiled.unit.document;
		document
			.descendants(document.root())
			.filter(|(_, element)| matches!(element.kind, ElementKind::Media { .. }))
			.map(|(_, element)| element.kind.clone())
			.collect()
	}

	fn medium(kind: MediaKind, url: &str, alt: &str, width: Option<&str>) -> ElementKind {
		ElementKind::Media {
			kind,
			url: url.into(),
			alt: alt.into(),
			width: width.map(String::from),
		}
	}

	#[test]
	fn media_kinds_and_options() {
		let files = [("a.png", "png"), ("clip.webm", "webm"), ("s.mp3", "mp3")];
		assert_eq!(
			media(
				"![A *flower*](a.png) ![](clip.webm)[width=50%] ![x](s.mp3)",
				&files
			),
			[
				medium(MediaKind::Image, &copy("a.png", "png"), "A flower", None),
				medium(
					MediaKind::Video,
					&copy("clip.webm", "webm"),
					"",
					Some("50%")
				),
				medium(MediaKind::Audio, &copy("s.mp3", "mp3"), "x", None),
			]
		);
		assert_eq!(
			media("![](https://x.y/stream)[type=video]", &[]),
			[medium(MediaKind::Video, "https://x.y/stream", "", None)]
		);
		assert_eq!(
			media("![](data:image/png;base64,AAAA)[type=image]", &[]),
			[medium(
				MediaKind::Image,
				"data:image/png;base64,AAAA",
				"",
				None
			)]
		);
	}

	/// Where the file `path` with this content is copied.
	fn copy(path: &str, content: &str) -> String {
		asset_output_path(path, blake3::hash(content.as_bytes()).to_hex().as_str())
	}

	#[test]
	fn workspace_files_are_assets() {
		// Wherever the media is written, the file is resolved from there, and copied under
		// `assets/` by its content: `c.png` has the same content as `a.png`.
		let compiled = compile_with(
			"![](img/a.png)\n:include parts/p\n",
			&[
				("img/a.png", "a"),
				(
					"parts/p.nml",
					"![](../img/b.png) ![](/img/a.png) ![](../img/c.png)",
				),
				("img/b.png", "b"),
				("img/c.png", "a"),
			],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let (a, b) = (copy("img/a.png", "a"), copy("img/b.png", "b"));
		assert!(a.starts_with("assets/") && a.ends_with(".png"), "{a}");
		assert_eq!(a.len(), "assets/".len() + 16 + ".png".len());
		assert_eq!(
			compiled.unit.assets,
			[
				Asset {
					path: "img/a.png".into(),
					output: a.clone()
				},
				Asset {
					path: "img/b.png".into(),
					output: b.clone()
				},
				Asset {
					path: "img/c.png".into(),
					output: a.clone()
				},
			]
		);
		let hash = blake3::hash(b"b").to_hex().to_string();
		assert!(compiled.unit.dependencies.contains(&Dependency::File {
			path: "img/b.png".into(),
			hash
		}));
		let urls: Vec<_> = media_of(&compiled)
			.into_iter()
			.filter_map(|kind| match kind {
				ElementKind::Media { url, .. } => Some(url),
				_ => None,
			})
			.collect();
		assert_eq!(urls, [a.clone(), b, a.clone(), a.clone()]);

		// URLs are relative to where the page is written.
		let compiled = compile_with(
			":set nml.output = guide/x/index.html\n\n![](img/a.png)",
			&[("img/a.png", "a")],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		assert_eq!(
			media_of(&compiled),
			[medium(MediaKind::Image, &format!("../../{a}"), "", None)]
		);
	}

	#[test]
	fn media_errors() {
		let compiled = compile_with("![](missing.png)", &[]);
		assert_diagnostics!(compiled.diagnostics, [3008 @ 4..15]);
		let compiled = compile_with("![](notes.txt)", &[("notes.txt", "")]);
		assert_diagnostics!(compiled.diagnostics, [2013 @ 4..13]);
		let compiled = compile_with("![](a.png)[type=gif]", &[("a.png", "")]);
		assert_diagnostics!(compiled.diagnostics, [1007]);
	}

	#[test]
	fn figures() {
		let compiled = compile_with(
			":figure #flower A **flower**\n![](a.png)\n\n:figure\n```\ncode\n```\n:figure #b B\nText.",
			&[("a.png", "png")],
		);
		assert_diagnostics!(compiled.diagnostics, []);
		let document = &compiled.unit.document;
		assert_eq!(
			crate::tests::show(document, document.root()),
			r#"document[figure#flower[paragraph[media[]] caption["A " Bold["flower"]]] figure[code_block[]] figure#b[paragraph["Text."] caption["B"]]]"#
		);
		let exports = &compiled.unit.exports;
		assert_eq!(exports.len(), 2);
		assert_eq!(exports[0].property("title"), Some("A flower"));
		assert_eq!(exports[0].property("number"), Some("1"));
		assert_eq!(exports[0].property("media"), Some("a.png"));
		assert_eq!(exports[1].property("number"), Some("3"));
		assert_eq!(exports[1].property("media"), None);

		let compiled = compile_with(":figure #x X\n\nlater", &[]);
		assert_diagnostics!(compiled.diagnostics, [1003]);
	}

	#[test]
	fn external_urls() {
		for (url, external) in [
			("https://x.y/a.png", true),
			("//cdn.example/a.png", true),
			("data:image/png;base64,AA", true),
			("mailto:a@b.c", true),
			("a.png", false),
			("img/a:b.png", false),
			("../a.png", false),
			("c:/a.png", false),
		] {
			assert_eq!(super::is_external(url), external, "{url}");
		}
	}
}
