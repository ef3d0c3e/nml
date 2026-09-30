//! External renderers: programs that turn LaTeX, Graphviz or Typst source into SVG.

use std::fmt;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

/// How long a renderer may run.
const TIME_LIMIT: Duration = Duration::from_secs(60);

/// The programs that render, and their settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renderers {
	pub latex: String,
	pub dvisvgm: String,
	pub dot: String,
	pub typst: String,
	/// LaTeX environments by name: `main` is used when none is named.
	pub tex_environments: Vec<(String, TexEnvironment)>,
}

impl Default for Renderers {
	fn default() -> Self {
		Self {
			latex: "latex".into(),
			dvisvgm: "dvisvgm".into(),
			dot: "dot".into(),
			typst: "typst".into(),
			tex_environments: Vec::new(),
		}
	}
}

/// A LaTeX environment: what comes before the content, and the program.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TexEnvironment {
	/// Inserted before `\begin{document}`: packages and macros.
	pub preamble: String,
	/// The font size in points.
	pub font_size: Option<u32>,
	/// Inserted before the source of LaTeX blocks (not math), on a line of its own.
	pub block_prepend: String,
	/// The LaTeX program, instead of [`Renderers::latex`].
	pub latex: Option<String>,
}

/// What to render.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Job {
	/// LaTeX math, inline or displayed.
	Math {
		source: String,
		display: bool,
	},
	/// A LaTeX document body, such as a TikZ picture.
	Tex {
		source: String,
		environment: String,
	},
	/// Graphviz, with a layout engine (`dot`, `neato`, …).
	Graph {
		source: String,
		layout: String,
	},
	Typst {
		source: String,
	},
}

/// Why a job did not render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderError {
	/// The program could not be started.
	Missing { program: String, message: String },
	/// The program failed: its message.
	Failed { program: String, message: String },
	/// The program ran out of time.
	TimedOut { program: String },
	/// No LaTeX environment of that name.
	UnknownEnvironment { name: String },
}

impl fmt::Display for RenderError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			RenderError::Missing { program, message } => {
				write!(f, "cannot run `{program}`: {message}")
			}
			RenderError::Failed { program, message } => write!(f, "`{program}` failed: {message}"),
			RenderError::TimedOut { program } => write!(f, "`{program}` took too long"),
			RenderError::UnknownEnvironment { name } => {
				write!(
					f,
					"`{name}` is not a LaTeX environment of the configuration"
				)
			}
		}
	}
}

impl std::error::Error for RenderError {}

/// The version of what [`Renderers::latex`] makes of `dvisvgm`'s output, in the cache key:
/// renders cached by an nml that made something else are made again.
const LATEX_OUTPUT: u32 = 2;

/// The size of LaTeX's text when no environment sets it, in points: the `standalone` class's.
const LATEX_FONT_SIZE: f64 = 10.0;

impl Renderers {
	fn environment(&self, name: &str) -> Result<TexEnvironment, RenderError> {
		match self.tex_environments.iter().find(|(env, _)| env == name) {
			Some((_, environment)) => Ok(environment.clone()),
			None if name == "main" => Ok(TexEnvironment::default()),
			None => Err(RenderError::UnknownEnvironment { name: name.into() }),
		}
	}

	/// What the output of a job depends on besides its source: the programs and the LaTeX
	/// environment. Part of its cache key.
	pub fn environment_key(&self, job: &Job) -> Result<String, RenderError> {
		Ok(match job {
			Job::Math { .. } => self.tex_document_key("main", false)?,
			Job::Tex { environment, .. } => self.tex_document_key(environment, true)?,
			Job::Graph { layout, .. } => format!("{}\0{layout}", self.dot),
			Job::Typst { .. } => self.typst.clone(),
		})
	}

	fn tex_document_key(&self, environment: &str, block: bool) -> Result<String, RenderError> {
		let env = self.environment(environment)?;
		let prepend = if block {
			env.block_prepend.as_str()
		} else {
			""
		};
		Ok(format!(
			"{LATEX_OUTPUT}\0{}\0{}\0{}\0{:?}\0{prepend}",
			env.latex.as_deref().unwrap_or(&self.latex),
			self.dvisvgm,
			env.preamble,
			env.font_size
		))
	}

	/// Renders a job to SVG, without its XML prologue.
	pub fn render(&self, job: &Job) -> Result<String, RenderError> {
		let svg = match job {
			Job::Math { source, display } => {
				let body = if *display {
					format!("\\[{source}\\]")
				} else {
					format!("${source}$")
				};
				self.latex(&body, "main", false)?
			}
			Job::Tex {
				source,
				environment,
			} => self.latex(source, environment, true)?,
			Job::Graph { source, layout } => run(
				Command::new(&self.dot)
					.arg(format!("-K{layout}"))
					.arg("-Tsvg"),
				&self.dot,
				source.as_bytes(),
			)?,
			Job::Typst { source } => {
				let document = format!(
					"#set page(width: auto, height: auto, margin: 0pt, fill: none)\n{source}"
				);
				run(
					Command::new(&self.typst).args(["compile", "--format", "svg", "-", "-"]),
					&self.typst,
					document.as_bytes(),
				)?
			}
		};
		Ok(strip_prologue(&svg).to_string())
	}

	/// The standalone LaTeX document of `body` in an environment. A block gets the
	/// environment's `block_prepend` first.
	fn tex_document(env: &TexEnvironment, body: &str, block: bool) -> String {
		let size = env
			.font_size
			.map(|size| format!(",{size}pt"))
			.unwrap_or_default();
		let prepend = match block && !env.block_prepend.is_empty() {
			true => format!("{}\n", env.block_prepend),
			false => String::new(),
		};
		format!(
			"\\documentclass[preview{size}]{{standalone}}\n{}\n\\begin{{document}}\n{prepend}{body}\n\\end{{document}}\n",
			env.preamble
		)
	}

	/// LaTeX in a standalone document, through the environment's LaTeX program then
	/// `dvisvgm`, in a directory of its own.
	fn latex(&self, body: &str, environment: &str, block: bool) -> Result<String, RenderError> {
		let env = self.environment(environment)?;
		let document = Self::tex_document(&env, body, block);
		let latex = env.latex.as_deref().unwrap_or(&self.latex);
		let dir = TempDir::new()?;
		std::fs::write(dir.path().join("doc.tex"), document).map_err(|error| {
			RenderError::Failed {
				program: latex.to_string(),
				message: error.to_string(),
			}
		})?;
		let log = run(
			Command::new(latex)
				.args(["-interaction=nonstopmode", "-halt-on-error", "doc.tex"])
				.current_dir(dir.path()),
			latex,
			b"",
		);
		if let Err(RenderError::Failed { program, message }) = log {
			// LaTeX reports its errors on lines starting with `!`.
			let error: Vec<_> = message
				.lines()
				.skip_while(|line| !line.starts_with('!'))
				.take(3)
				.collect();
			let message = if error.is_empty() {
				message
			} else {
				error.join("\n")
			};
			return Err(RenderError::Failed { program, message });
		}
		log?;
		// The preview package's box keeps the baseline, which inline math is aligned on;
		// blocks are cut to what they draw.
		let mut args = vec!["--no-fonts", "--stdout"];
		if block {
			args.push("--exact-bbox");
		}
		args.push("doc.dvi");
		let (svg, messages) = run_with_messages(
			Command::new(&self.dvisvgm)
				.args(args)
				.current_dir(dir.path()),
			&self.dvisvgm,
		)?;
		let font_size = env.font_size.map_or(LATEX_FONT_SIZE, f64::from);
		// A block is not in a line: it has no baseline to align.
		let messages = if block { "" } else { messages.as_str() };
		Ok(sized_in_em(&double_quoted(&svg), messages, font_size))
	}
}

/// Runs a program with `input` on its standard input, and returns its standard output. Its
/// standard error (or output) is the message of a failure.
fn run(command: &mut Command, program: &str, input: &[u8]) -> Result<String, RenderError> {
	run_full(command, program, input).map(|(out, _)| out)
}

/// Runs a program without input, and returns its standard output and error.
fn run_with_messages(
	command: &mut Command,
	program: &str,
) -> Result<(String, String), RenderError> {
	run_full(command, program, b"")
}

fn run_full(
	command: &mut Command,
	program: &str,
	input: &[u8],
) -> Result<(String, String), RenderError> {
	let mut child = command
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.map_err(|error| RenderError::Missing {
			program: program.into(),
			message: error.to_string(),
		})?;
	let mut stdin = child.stdin.take().unwrap();
	let input = input.to_vec();
	// Written and read on threads of their own, so that full pipes cannot block the child.
	let writer = std::thread::spawn(move || {
		let _ = stdin.write_all(&input);
	});
	let mut stdout = child.stdout.take().unwrap();
	let reader = std::thread::spawn(move || {
		let mut out = Vec::new();
		let _ = stdout.read_to_end(&mut out);
		out
	});
	let mut stderr = child.stderr.take().unwrap();
	let errors = std::thread::spawn(move || {
		let mut out = Vec::new();
		let _ = stderr.read_to_end(&mut out);
		out
	});
	let start = Instant::now();
	let status = loop {
		match child.try_wait() {
			Ok(Some(status)) => break status,
			Ok(None) if start.elapsed() > TIME_LIMIT => {
				let _ = child.kill();
				let _ = child.wait();
				return Err(RenderError::TimedOut {
					program: program.into(),
				});
			}
			Ok(None) => std::thread::sleep(Duration::from_millis(5)),
			Err(error) => {
				return Err(RenderError::Failed {
					program: program.into(),
					message: error.to_string(),
				});
			}
		}
	};
	let _ = writer.join();
	let out = String::from_utf8_lossy(&reader.join().unwrap_or_default()).into_owned();
	let errors = String::from_utf8_lossy(&errors.join().unwrap_or_default()).into_owned();
	if status.success() {
		Ok((out, errors))
	} else {
		let message = if errors.trim().is_empty() {
			out
		} else {
			errors
		};
		Err(RenderError::Failed {
			program: program.into(),
			message: message.trim().to_string(),
		})
	}
}

/// A directory removed when dropped.
struct TempDir(std::path::PathBuf);

impl TempDir {
	fn new() -> Result<Self, RenderError> {
		use std::sync::atomic::AtomicU64;
		use std::sync::atomic::Ordering;
		static COUNT: AtomicU64 = AtomicU64::new(0);
		let name = format!(
			"nml-render-{}-{}",
			std::process::id(),
			COUNT.fetch_add(1, Ordering::Relaxed)
		);
		let path = std::env::temp_dir().join(name);
		std::fs::create_dir_all(&path).map_err(|error| RenderError::Failed {
			program: "nml".into(),
			message: error.to_string(),
		})?;
		Ok(Self(path))
	}

	fn path(&self) -> &Path {
		&self.0
	}
}

impl Drop for TempDir {
	fn drop(&mut self) {
		let _ = std::fs::remove_dir_all(&self.0);
	}
}

/// The SVG element, without the XML declaration, doctype and comments before it.
fn strip_prologue(svg: &str) -> &str {
	match svg.find("<svg") {
		Some(start) => svg[start..].trim_end(),
		None => svg.trim(),
	}
}

/// Makes an SVG draw in the color of the text around it, so that it can be read in any
/// color scheme. Black strokes and fills become `currentColor`, the shapes without a fill
/// (LaTeX's glyphs, Graphviz's text) inherit it, and Graphviz's white page is removed. Other
/// colors stay.
pub fn recolor(svg: &str) -> String {
	let mut out = svg.to_string();
	for attribute in ["fill", "stroke"] {
		for black in ["black", "#000000", "#000"] {
			out = out.replace(
				&format!(" {attribute}=\"{black}\""),
				&format!(" {attribute}=\"currentColor\""),
			);
		}
	}
	// Graphviz draws the page first in the graph's group.
	let page = "<polygon fill=\"white\" stroke=\"none\"";
	if let Some(group) = out.find(" class=\"graph\"")
		&& let Some(end) = out[group..].find('>')
	{
		let start = group + end + 1;
		let first = start + (out[start..].len() - out[start..].trim_start().len());
		if out[first..].starts_with(page) {
			out.replace_range(
				first..first + page.len(),
				"<polygon fill=\"none\" stroke=\"none\"",
			);
		}
	}
	// SVG's default fill is black.
	if let Some(start) = out.find("<svg")
		&& let Some(end) = out[start..].find('>')
		&& !out[start..start + end].contains(" fill=")
	{
		out.insert_str(start + 4, " fill=\"currentColor\"");
	}
	out
}

/// `dvisvgm`'s attributes, written `a='v'`, as `a="v"`: as the other programs write them,
/// and as [`scope_ids`] and [`recolor`] read them.
fn double_quoted(svg: &str) -> String {
	let mut out = String::with_capacity(svg.len());
	let mut rest = svg;
	while let Some(start) = rest.find("='") {
		let value = &rest[start + 2..];
		match value.find('\'') {
			Some(end) if !value[..end].contains('"') => {
				out.push_str(&rest[..start]);
				out.push_str("=\"");
				out.push_str(&value[..end]);
				out.push('"');
				rest = &value[end + 1..];
			}
			_ => {
				out.push_str(&rest[..start + 2]);
				rest = value;
			}
		}
	}
	out.push_str(rest);
	out
}

/// A number of `dvisvgm`'s messages, in TeX's points: `name=12.5pt`.
fn measure(messages: &str, name: &str) -> Option<f64> {
	let start = messages.find(&format!("{name}="))? + name.len() + 1;
	let value = &messages[start..];
	value[..value.find("pt")?].parse().ok()
}

/// A LaTeX SVG sized in `em`, so that it scales with the text around it, and aligned on its
/// baseline, which is `depth` above its bottom. The SVG's points are PostScript's, 1/72 in;
/// the messages' and the font size's are TeX's, 1/72.27 in.
fn sized_in_em(svg: &str, messages: &str, font_size: f64) -> String {
	const TEX_POINTS: f64 = 72.27 / 72.0;
	let attribute = |name: &str| -> Option<(std::ops::Range<usize>, f64)> {
		let tag = &svg[svg.find("<svg")?..];
		let tag = &tag[..tag.find('>')?];
		let key = format!(" {name}=\"");
		let start = svg.find("<svg")? + tag.find(&key)? + key.len();
		let end = start + svg[start..].find('"')?;
		let value = svg[start..end].strip_suffix("pt")?.parse().ok()?;
		Some((start..end, value))
	};
	let (Some((width_at, width)), Some((height_at, height))) =
		(attribute("width"), attribute("height"))
	else {
		return svg.to_string();
	};
	let em = |tex_points: f64| format!("{:.4}em", tex_points / font_size);
	// Replaced from the end, so that the other range stays valid.
	let mut out = svg.to_string();
	let mut ranges = [
		(width_at, em(width * TEX_POINTS)),
		(height_at, em(height * TEX_POINTS)),
	];
	ranges.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
	for (range, value) in ranges {
		out.replace_range(range, &value);
	}
	if let Some(depth) = measure(messages, "depth").filter(|depth| *depth > 0.0) {
		let style = format!(" style=\"vertical-align: -{}\"", em(depth));
		out.insert_str(out.find("<svg").unwrap() + 4, &style);
	}
	out
}

/// Prefixes the ids of an SVG, and the references to them, so that several SVGs can share a
/// page: `id="g1"` becomes `id="p-g1"`, `#g1` becomes `#p-g1`.
pub fn scope_ids(svg: &str, prefix: &str) -> String {
	let mut ids = Vec::new();
	let mut rest = svg;
	while let Some(start) = rest.find(" id=\"") {
		rest = &rest[start + 5..];
		if let Some(end) = rest.find('"') {
			ids.push(rest[..end].to_string());
			rest = &rest[end..];
		}
	}
	if ids.is_empty() {
		return svg.to_string();
	}
	// Longest first, so that `g1` does not rewrite part of `g10`.
	ids.sort_by_key(|id| std::cmp::Reverse(id.len()));
	let mut out = svg.to_string();
	for id in &ids {
		for (from, to) in [
			(format!(" id=\"{id}\""), format!(" id=\"{prefix}-{id}\"")),
			(format!("\"#{id}\""), format!("\"#{prefix}-{id}\"")),
			(format!("url(#{id})"), format!("url(#{prefix}-{id})")),
		] {
			out = out.replace(&from, &to);
		}
	}
	out
}

#[cfg(test)]
mod tests {
	use super::*;

	fn has(program: &str) -> bool {
		Command::new(program)
			.arg("--version")
			.stdout(Stdio::null())
			.stderr(Stdio::null())
			.status()
			.is_ok()
	}

	#[test]
	fn graphviz() {
		if !has("dot") {
			eprintln!("skipped: no `dot`");
			return;
		}
		let renderers = Renderers::default();
		let svg = renderers
			.render(&Job::Graph {
				source: "digraph { a -> b }".into(),
				layout: "dot".into(),
			})
			.unwrap();
		assert!(svg.starts_with("<svg"), "{svg}");
		assert!(svg.ends_with("</svg>"), "{svg}");
		let error = renderers
			.render(&Job::Graph {
				source: "digraph { a -> }".into(),
				layout: "dot".into(),
			})
			.unwrap_err();
		assert!(matches!(error, RenderError::Failed { .. }), "{error}");
		assert!(error.to_string().contains("syntax error"), "{error}");
	}

	#[test]
	fn missing_programs() {
		let renderers = Renderers {
			dot: "nml-no-such-program".into(),
			..Renderers::default()
		};
		let error = renderers
			.render(&Job::Graph {
				source: String::new(),
				layout: "dot".into(),
			})
			.unwrap_err();
		assert!(matches!(error, RenderError::Missing { .. }), "{error}");
	}

	#[test]
	fn latex() {
		if !has("latex") || !has("dvisvgm") {
			eprintln!("skipped: no `latex` or `dvisvgm`");
			return;
		}
		let renderers = Renderers::default();
		let svg = renderers
			.render(&Job::Math {
				source: "e^{i\\pi} = -1".into(),
				display: false,
			})
			.unwrap();
		assert!(svg.starts_with("<svg"), "{svg}");
		let error = renderers
			.render(&Job::Math {
				source: "\\nope".into(),
				display: true,
			})
			.unwrap_err();
		assert!(
			error.to_string().contains("Undefined control sequence"),
			"{error}"
		);
	}

	/// `dvisvgm`'s output for inline math, cut short, and its messages.
	const DVISVGM: &str = "<?xml version='1.0' encoding='UTF-8'?>
<!-- This file was generated by dvisvgm 3.4 -->
<svg version='1.1' xmlns='http://www.w3.org/2000/svg' xmlns:xlink='http://www.w3.org/1999/xlink' width='20pt' height='12pt' viewBox='-72 -72 20 12'>
<defs>
<path id='g0-101' d='M1 1'/>
<path id='g0-10' d='M2 2'/>
</defs>
<g id='page1'>
<use x='-72' y='-64' xlink:href='#g0-101'/>
<use x='-66' y='-64' xlink:href='#g0-10'/>
</g>
</svg>";
	const MESSAGES: &str = "pre-processing DVI file (format version 2)
processing page 1
  computing extents based on data set by preview package (version 13.2)
  width=20.075pt, height=9pt, depth=3pt
  graphic size: 20.075pt x 12pt (20bp x 11.955bp)
";

	#[test]
	fn latex_output() {
		let svg = double_quoted(strip_prologue(DVISVGM));
		assert!(svg.starts_with("<svg version=\"1.1\" xmlns=\"http://www.w3.org/2000/svg\""));
		assert!(!svg.contains('\''), "{svg}");
		// Then its ids are scoped, and its glyphs drawn in the text's color.
		let svg = recolor(&scope_ids(&svg, "k"));
		assert!(svg.contains("<path id=\"k-g0-101\""), "{svg}");
		assert!(svg.contains("<path id=\"k-g0-10\""), "{svg}");
		assert!(svg.contains("xlink:href=\"#k-g0-101\""), "{svg}");
		assert!(svg.contains("xlink:href=\"#k-g0-10\""), "{svg}");
		assert!(svg.starts_with("<svg fill=\"currentColor\""), "{svg}");

		// In em of a 10pt font, and on the baseline, 3pt above its bottom.
		let svg = sized_in_em(&svg, MESSAGES, 10.0);
		let tag = &svg[..svg.find('>').unwrap()];
		assert!(
			tag.contains(" width=\"2.0075em\" height=\"1.2045em\""),
			"{tag}"
		);
		assert!(
			tag.starts_with("<svg style=\"vertical-align: -0.3000em\""),
			"{tag}"
		);
		// Other sizes, and no depth: not aligned.
		let svg = sized_in_em(&double_quoted(DVISVGM), "", 12.0);
		assert!(
			svg.contains(" width=\"1.6729em\" height=\"1.0037em\""),
			"{svg}"
		);
		assert!(!svg.contains("vertical-align"), "{svg}");
		// Sizes that are not in points stay.
		let svg = "<svg width=\"10px\" height=\"2px\"></svg>";
		assert_eq!(sized_in_em(svg, MESSAGES, 10.0), svg);

		assert_eq!(measure(MESSAGES, "depth"), Some(3.0));
		assert_eq!(measure(MESSAGES, "nope"), None);
		assert_eq!(
			double_quoted("<a b='it\"s' c='d'>"),
			"<a b='it\"s' c=\"d\">"
		);
	}

	#[test]
	fn typst() {
		if !has("typst") {
			eprintln!("skipped: no `typst`");
			return;
		}
		let svg = Renderers::default()
			.render(&Job::Typst {
				source: "$ a^2 $".into(),
			})
			.unwrap();
		assert!(svg.starts_with("<svg"), "{svg}");
	}

	#[test]
	fn block_prepend_and_program() {
		let env = TexEnvironment {
			preamble: "\\usepackage{tikz}".into(),
			font_size: Some(11),
			block_prepend: "\\centering".into(),
			latex: Some("nml-no-such-latex".into()),
		};
		// Only blocks get `block_prepend`.
		assert_eq!(
			Renderers::tex_document(&env, "x", true),
			"\\documentclass[preview,11pt]{standalone}\n\\usepackage{tikz}\n\
			\\begin{document}\n\\centering\nx\n\\end{document}\n"
		);
		assert!(!Renderers::tex_document(&env, "x", false).contains("centering"));
		let renderers = Renderers {
			tex_environments: vec![("own".into(), env.clone())],
			..Renderers::default()
		};
		// The environment's program runs, and is part of the key, as `block_prepend` is.
		let job = Job::Tex {
			source: "x".into(),
			environment: "own".into(),
		};
		let error = renderers.render(&job).unwrap_err();
		assert!(
			matches!(&error, RenderError::Missing { program, .. } if program == "nml-no-such-latex"),
			"{error}"
		);
		let key = renderers.environment_key(&job).unwrap();
		let other = Renderers {
			tex_environments: vec![(
				"own".into(),
				TexEnvironment {
					block_prepend: String::new(),
					..env.clone()
				},
			)],
			..Renderers::default()
		};
		assert_ne!(other.environment_key(&job).unwrap(), key);
		let other = Renderers {
			tex_environments: vec![("own".into(), TexEnvironment { latex: None, ..env })],
			..Renderers::default()
		};
		assert_ne!(other.environment_key(&job).unwrap(), key);
	}

	#[test]
	fn environments() {
		let renderers = Renderers {
			tex_environments: vec![(
				"tikz".into(),
				TexEnvironment {
					preamble: "\\usepackage{tikz}".into(),
					font_size: Some(12),
					..TexEnvironment::default()
				},
			)],
			..Renderers::default()
		};
		let tikz = Job::Tex {
			source: String::new(),
			environment: "tikz".into(),
		};
		let math = Job::Math {
			source: String::new(),
			display: false,
		};
		assert_ne!(
			renderers.environment_key(&tikz).unwrap(),
			renderers.environment_key(&math).unwrap()
		);
		let unknown = Job::Tex {
			source: String::new(),
			environment: "nope".into(),
		};
		assert_eq!(
			renderers.environment_key(&unknown),
			Err(RenderError::UnknownEnvironment {
				name: "nope".into()
			})
		);
	}

	#[test]
	fn recolored() {
		// Graphviz.
		let svg = r#"<svg width="62pt" height="116pt">
<g id="x-graph0" class="graph" transform="scale(1 1)">
<polygon fill="white" stroke="none" points="-4,4 -4,-112"/>
<ellipse fill="none" stroke="black" cx="27"/>
<ellipse fill="white" stroke="none" cx="1"/>
<text text-anchor="middle">a</text>
<polygon fill="black" stroke="black" points="30.5,-47.62"/>
<ellipse fill="none" stroke="red" cx="99"/>
</g>
</svg>"#;
		assert_eq!(
			recolor(svg),
			r#"<svg fill="currentColor" width="62pt" height="116pt">
<g id="x-graph0" class="graph" transform="scale(1 1)">
<polygon fill="none" stroke="none" points="-4,4 -4,-112"/>
<ellipse fill="none" stroke="currentColor" cx="27"/>
<ellipse fill="white" stroke="none" cx="1"/>
<text text-anchor="middle">a</text>
<polygon fill="currentColor" stroke="currentColor" points="30.5,-47.62"/>
<ellipse fill="none" stroke="red" cx="99"/>
</g>
</svg>"#
		);
		// A graph with a background of its own keeps it.
		let svg = r#"<svg><g class="graph"><polygon fill="lightgrey" stroke="none"/></g></svg>"#;
		assert_eq!(
			recolor(svg),
			r#"<svg fill="currentColor"><g class="graph"><polygon fill="lightgrey" stroke="none"/></g></svg>"#
		);
		// Typst and dvisvgm.
		assert_eq!(
			recolor(r##"<svg class="typst-doc"><path fill="#000000" stroke="#000"/></svg>"##),
			r#"<svg fill="currentColor" class="typst-doc"><path fill="currentColor" stroke="currentColor"/></svg>"#
		);
		assert_eq!(
			recolor(
				r##"<svg height="9pt"><g id="page1"><use x="1" xlink:href="#g0-1"/></g></svg>"##
			),
			r##"<svg fill="currentColor" height="9pt"><g id="page1"><use x="1" xlink:href="#g0-1"/></g></svg>"##
		);
		// A fill of the SVG's own stays.
		assert_eq!(
			recolor(r#"<svg fill="blue"><path/></svg>"#),
			r#"<svg fill="blue"><path/></svg>"#
		);
		// Idempotent: cached SVGs may be recolored again.
		assert_eq!(recolor(&recolor(svg)), recolor(svg));
	}

	#[test]
	fn graphviz_is_recolored() {
		if !has("dot") {
			eprintln!("skipped: no `dot`");
			return;
		}
		let svg = Renderers::default()
			.render(&Job::Graph {
				source: "digraph { a -> b }".into(),
				layout: "dot".into(),
			})
			.unwrap();
		let svg = recolor(&svg);
		assert!(svg.starts_with("<svg fill=\"currentColor\""), "{svg}");
		assert!(!svg.contains("\"black\""), "{svg}");
		assert!(!svg.contains("\"white\""), "{svg}");
	}

	#[test]
	fn ids_are_scoped() {
		let svg = r##"<svg><defs><path id="g1"/><path id="g10"/></defs><use xlink:href="#g1"/><use href="#g10"/><rect fill="url(#g1)"/></svg>"##;
		assert_eq!(
			scope_ids(svg, "x"),
			r##"<svg><defs><path id="x-g1"/><path id="x-g10"/></defs><use xlink:href="#x-g1"/><use href="#x-g10"/><rect fill="url(#x-g1)"/></svg>"##
		);
		assert_eq!(scope_ids("<svg/>", "x"), "<svg/>");
	}

	#[test]
	fn prologues_are_stripped() {
		assert_eq!(
			strip_prologue("<?xml version=\"1.0\"?>\n<!DOCTYPE svg>\n<svg/>\n"),
			"<svg/>"
		);
	}
}
