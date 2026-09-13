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
			"{}\0{}\0{}\0{:?}\0{prepend}",
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
		run(
			Command::new(&self.dvisvgm)
				.args(["--no-fonts", "--exact-bbox", "--stdout", "doc.dvi"])
				.current_dir(dir.path()),
			&self.dvisvgm,
			b"",
		)
	}
}

/// Runs a program with `input` on its standard input, and returns its standard output. Its
/// standard error (or output) is the message of a failure.
fn run(command: &mut Command, program: &str, input: &[u8]) -> Result<String, RenderError> {
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
		Ok(out)
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
