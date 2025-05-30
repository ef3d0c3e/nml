use std::cell::RefCell;
use std::io::Read;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Once;

use crypto::digest::Digest;
use crypto::sha2::Sha512;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;

use crate::cache::cache::Cached;
use crate::cache::cache::CachedError;
use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target::HTML;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ContainerElement;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::element::LinkableElement;
use crate::unit::element::ReferenceableElement;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;
use crate::unit::variable::VariableName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TexKind {
	Block,
	Inline,
}

impl From<TexKind> for ElemKind {
	fn from(value: TexKind) -> Self {
		match value {
			TexKind::Block => ElemKind::Block,
			TexKind::Inline => ElemKind::Inline,
		}
	}
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

#[derive(Debug)]
pub struct Latex {
	pub(crate) location: Token,
	pub(crate) mathmode: bool,
	pub(crate) kind: TexKind,
	pub(crate) env: String,
	pub(crate) tex: String,
	pub(crate) caption: Option<String>,
}

struct FormattedTex(String);

impl Cached for FormattedTex {
	type Key = String;
	type Value = String;

	fn sql_table() -> &'static str {
		"CREATE TABLE IF NOT EXISTS cached_latex (
				digest TEXT PRIMARY KEY,
				svg    BLOB NOT NULL);"
	}

	fn sql_get_query() -> &'static str {
		"SELECT svg FROM cached_latex WHERE digest = (?1)"
	}

	fn sql_insert_query() -> &'static str {
		"INSERT INTO cached_latex (digest, svg) VALUES (?1, ?2)"
	}

	fn key(&self) -> <Self as Cached>::Key {
		let mut hasher = Sha512::new();
		hasher.input(self.0.as_bytes());
		hasher.result_str()
	}
}

fn format_latex(fontsize: &str, preamble: &str, tex: &str) -> FormattedTex {
	FormattedTex(format!(
		r"\documentclass[preview]{{standalone}}
{preamble}
\begin{{document}}
\begin{{preview}}
{tex}
\end{{preview}}
\end{{document}}"
	))
}

fn latex_to_svg(tex: &FormattedTex, exec: &String, fontsize: &String) -> Result<String, String> {
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

	if let Err(e) = process.stdin.unwrap().write_all(tex.0.as_bytes()) {
		panic!("Unable to write to `latex2svg`'s stdin: {}", e);
	}

	let mut result = String::new();
	if let Err(e) = process.stdout.unwrap().read_to_string(&mut result) {
		panic!("Unable to read `latex2svg` stdout: {}", e)
	}

	Ok(result)
}

impl Element for Latex {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		self.kind.into()
	}

	fn element_name(&self) -> &'static str {
		"Latex"
	}

	fn compile<'e>(
		&'e self,
		scope: Rc<RefCell<Scope>>,
		compiler: &'e Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				static CACHE_INIT: Once = Once::new();

				CACHE_INIT.call_once(|| {
					let cache = compiler.get_cache();
					let con = tokio::runtime::Runtime::new()
						.unwrap()
						.block_on(cache.get_connection());
					if let Err(e) = FormattedTex::init(&con) {
						eprintln!("Unable to create cache table: {e}");
					}
				});

				let exec = scope
					.get_variable(&VariableName(format!("latex.{}.exec", self.env)))
					.map_or("latex2svg".to_string(), |(var, _)| var.to_string());
				let fontsize = scope
					.get_variable(&VariableName(format!("latex.{}.fontsize", self.env)))
					.map_or("12".to_string(), |(var, _)| var.to_string());
				let preamble = scope
					.get_variable(&VariableName(format!("latex.{}.preamble", self.env)))
					.map_or("".to_string(), |(var, _)| var.to_string());
				let prepend = if self.mathmode {
					"".to_string()
				} else {
					scope
						.get_variable(&VariableName(format!("tex.{}.block_prepend", self.env)))
						.map_or("".to_string(), |(var, _)| var.to_string() + "\n")
				};

				let latex = if self.mathmode {
					format_latex(&fontsize, &preamble, &format!("${{{}}}$", self.tex))
				} else {
					format_latex(&fontsize, &preamble, &format!("{prepend}{}", self.tex))
				};

				let sanitizer = compiler.sanitizer();
				let location = self.location().clone();
				let caption = self.caption.clone();
				let cache = compiler.get_cache();
				let fut = async move {
					let con = cache.get_connection().await;
					let mut result = match latex.cached(&con, |s| latex_to_svg(s, &exec, &fontsize))
					{
						Ok(s) => s,
						Err(CachedError::SqlErr(e)) => {
							return Err(compile_err!(
								location,
								"Failed to process LaTeX element".to_string(),
								format!("Querying the cache failed: {e}")
							))
						}
						Err(CachedError::GenErr(e)) => {
							return Err(compile_err!(
								location,
								"Failed to process LaTeX element".to_string(),
								e
							))
						}
					};

					// Caption
					if let (Some(caption), Some(start)) = (&caption, result.find('>')) {
						result.insert_str(
							start + 1,
							format!("<title>{}</title>", sanitizer.sanitize(caption)).as_str(),
						);
					}
					Ok(result)
				};
				output.add_task(self.location.clone(), "Latex".into(), Box::pin(fut));
			}
			_ => todo!(),
		}
		Ok(())
	}

	fn as_referenceable(self: Rc<Self>) -> Option<Rc<dyn ReferenceableElement>> {
		None
	}
	fn as_linkable(self: Rc<Self>) -> Option<Rc<dyn LinkableElement>> {
		None
	}
	fn as_container(self: Rc<Self>) -> Option<Rc<dyn ContainerElement>> {
		None
	}
}
