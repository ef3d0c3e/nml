use std::io::Read;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;
use std::str::FromStr;
use std::sync::Once;

use crate::cache::cache::Cached;
use crate::compiler::compiler::CompilerOutput;
use crate::compiler::compiler::Target::HTML;
use crate::parser::reports::Report;
use crypto::digest::Digest;
use crypto::sha2::Sha512;

use crate::cache::cache::CachedError;
use crate::compiler::compiler::Compiler;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::source::Token;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;

#[derive(Debug, PartialEq, Eq)]
pub enum TexKind {
	Block,
	Inline,
}

impl FromStr for TexKind {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"inline" => Ok(TexKind::Inline),
			"block" => Ok(TexKind::Block),
			_ => Err(format!("Unknown kind: {s}")),
		}
	}
}

impl From<&TexKind> for ElemKind {
	fn from(value: &TexKind) -> Self {
		match value {
			TexKind::Inline => ElemKind::Inline,
			_ => ElemKind::Block,
		}
	}
}

#[derive(Debug)]
pub struct Tex {
	pub(crate) location: Token,
	pub(crate) mathmode: bool,
	pub(crate) kind: TexKind,
	pub(crate) env: String,
	pub(crate) tex: String,
	pub(crate) caption: Option<String>,
}

impl Tex {
	fn format_latex(fontsize: &String, preamble: &String, tex: &String) -> FormattedTex {
		FormattedTex(format!(
			r"\documentclass[{}pt,preview]{{standalone}}
{}
\begin{{document}}
\begin{{preview}}
{}
\end{{preview}}
\end{{document}}",
			fontsize, preamble, tex
		))
	}
}

struct FormattedTex(String);

impl FormattedTex {
	/// Renders latex to svg
	fn latex_to_svg(&self, exec: &String, fontsize: &String) -> Result<String, String> {
		print!("Rendering LaTex `{}`... ", self.0);
		let process = match Command::new(exec)
			.arg("--fontsize")
			.arg(fontsize)
			.stdout(Stdio::piped())
			.stdin(Stdio::piped())
			.spawn()
		{
			Err(e) => return Err(format!("Could not spawn `{exec}`: {}", e)),
			Ok(process) => process,
		};

		if let Err(e) = process.stdin.unwrap().write_all(self.0.as_bytes()) {
			panic!("Unable to write to `latex2svg`'s stdin: {}", e);
		}

		let mut result = String::new();
		if let Err(e) = process.stdout.unwrap().read_to_string(&mut result) {
			panic!("Unable to read `latex2svg` stdout: {}", e)
		}
		println!("Done!");

		Ok(result)
	}
}

impl Cached for FormattedTex {
	type Key = String;
	type Value = String;

	fn sql_table() -> &'static str {
		"CREATE TABLE IF NOT EXISTS cached_tex (
				digest TEXT PRIMARY KEY,
				svg    BLOB NOT NULL);"
	}

	fn sql_get_query() -> &'static str { "SELECT svg FROM cached_tex WHERE digest = (?1)" }

	fn sql_insert_query() -> &'static str { "INSERT INTO cached_tex (digest, svg) VALUES (?1, ?2)" }

	fn key(&self) -> <Self as Cached>::Key {
		let mut hasher = Sha512::new();
		hasher.input(self.0.as_bytes());

		hasher.result_str()
	}
}

impl Element for Tex {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { (&self.kind).into() }

	fn element_name(&self) -> &'static str { "LaTeX" }

	fn compile<'e>(
		&'e self,
		compiler: &'e Compiler,
		document: &'e dyn Document,
		output: &'e mut CompilerOutput<'e>,
	) -> Result<&'e mut CompilerOutput<'e>, Vec<Report>> {
		match compiler.target() {
			HTML => {
				static CACHE_INIT: Once = Once::new();
				CACHE_INIT.call_once(|| {
					if let Some(con) = compiler.cache() {
						if let Err(e) = FormattedTex::init(con) {
							eprintln!("Unable to create cache table: {e}");
						}
					}
				});

				let exec = document
					.get_variable(format!("tex.{}.exec", self.env).as_str())
					.map_or("latex2svg".to_string(), |var| var.to_string());
				// FIXME: Because fontsize is passed as an arg, verify that it cannot be used to execute python/shell code
				let fontsize = document
					.get_variable(format!("tex.{}.fontsize", self.env).as_str())
					.map_or("12".to_string(), |var| var.to_string());
				let preamble = document
					.get_variable(format!("tex.{}.preamble", self.env).as_str())
					.map_or("".to_string(), |var| var.to_string());
				let prepend = if self.mathmode {
					"".to_string()
				} else {
					document
						.get_variable(format!("tex.{}.block_prepend", self.env).as_str())
						.map_or("".to_string(), |var| var.to_string() + "\n")
				};

				let latex = if self.mathmode {
					Tex::format_latex(&fontsize, &preamble, &format!("${{{}}}$", self.tex))
				} else {
					Tex::format_latex(&fontsize, &preamble, &format!("{prepend}{}", self.tex))
				};

				let fut = async move
				{
					let result = if let Some(con) = compiler.cache() {
						match latex.cached(con, |s| s.latex_to_svg(&exec, &fontsize)) {
							Ok(s) => Ok(s),
							Err(e) => match e {
								CachedError::SqlErr(e) => {
									Err(format!("Querying the cache failed: {e}"))
								}
								CachedError::GenErr(e) => Err(e),
							},
						}
					} else {
						latex.latex_to_svg(&exec, &fontsize)
					};

					if let Err(e) = result {
						return Err(compile_err!(self.location(), "Failed to compile LaTeX element".into(), e));
					}

					// Caption
					let mut result = result.unwrap();
					if let (Some(caption), Some(start)) = (&self.caption, result.find('>')) {
						result.insert_str(
							start + 1,
							format!("<title>{}</title>", Compiler::sanitize(HTML, caption))
							.as_str(),
						);
					}
					Ok(result)
				};
				output.add_task(Box::new(fut));
			}
			_ => todo!("Unimplemented"),
		}
		Ok(output)
	}
}
