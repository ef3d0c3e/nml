use auto_userdata::auto_userdata;
use mlua::AnyUserData;
use mlua::Lua;
use std::fmt::Display;
use std::io::Read;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::Once;

use ariadne::Span;
use crypto::digest::Digest;
use crypto::sha2::Sha512;
use parking_lot::RwLock;
use serde::Deserialize;
use serde::Serialize;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;

use crate::cache::cache::Cached;
use crate::cache::cache::CachedError;
use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target::HTML;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;
use crate::unit::variable::VariableName;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum TypstKind {
	Block,
	Inline,
}

impl From<TypstKind> for ElemKind {
	fn from(value: TypstKind) -> Self {
		match value {
			TypstKind::Block => ElemKind::Block,
			TypstKind::Inline => ElemKind::Inline,
		}
	}
}

impl FromStr for TypstKind {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"inline" => Ok(TypstKind::Inline),
			"block" => Ok(TypstKind::Block),
			_ => Err(format!("Unknown kind: {s}")),
		}
	}
}

impl Display for TypstKind {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			TypstKind::Block => write!(f, "Block"),
			TypstKind::Inline => write!(f, "Inline"),
		}
	}
}

#[derive(Debug)]
#[auto_userdata(proxy = "TypstProxy", immutable, mutable)]
pub struct Typst {
	#[lua_ud]
	pub(crate) location: Token,
	pub(crate) mathmode: bool,
	#[lua_value]
	pub(crate) kind: TypstKind,
	pub(crate) env: String,
	pub(crate) typ: String,
	pub(crate) caption: Option<String>,
}

struct FormattedTyp(String);

impl Cached for FormattedTyp {
	type Key = String;
	type Value = String;

	fn sql_table() -> &'static str {
		"CREATE TABLE IF NOT EXISTS cached_typst(
			digest TEXT PRIMARY KEY,
			svg BLOB NOT NULL
		)"
	}

	fn sql_get_query() -> &'static str {
		"SELECT svg FROM cached_typst WHERE digest = (?1)"
	}

	fn sql_insert_query() -> &'static str {
		"INSERT INTO cached_typst (digest, svg) VALUES (?1, ?2)"
	}

	fn key(&self) -> <Self as Cached>::Key {
		let mut hasher = Sha512::new();
		hasher.input(self.0.as_bytes());
		hasher.result_str()
	}
}

fn format_typst(fontsize: &str, preamble: &str, typ: &str) -> FormattedTyp {
	FormattedTyp(format!(
		r"#set page(width: auto, height: auto, margin: 0pt, background: none)
#set text(size: {fontsize})
{preamble}
{typ}"
	))
}

fn typst_to_svg(typ: &FormattedTyp, exec: &String) -> Result<String, String> {
	let process = match Command::new(exec)
		.arg("compile")
		.arg("--format")
		.arg("svg")
		.arg("-")
		.arg("-")
		.stdout(Stdio::piped())
		.stdin(Stdio::piped())
		.spawn()
	{
		Err(e) => return Err(format!("Could not spawn `{exec}`: {}", e)),
		Ok(proc) => proc,
	};
	if let Err(e) = process.stdin.unwrap().write_all(typ.0.as_bytes()) {
		panic!("Unable to write to typst's stdin: {e}");
	}

	let mut result = String::new();
	if let Err(e) = process.stdout.unwrap().read_to_string(&mut result) {
		panic!("Unable to read typst's stdout: {e}");
	}

	Ok(result)
}

impl Element for Typst {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		self.kind.into()
	}

	fn element_name(&self) -> &'static str {
		"Typst"
	}

	fn compile(
		&self,
		scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
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
					if let Err(e) = FormattedTyp::init(&con) {
						eprintln!("Unable to create cache table: {e}");
					}
				});

				let exec = scope
					.get_variable(&VariableName(format!("typst.{}.exec", self.env)))
					.map_or("typst".to_string(), |(var, _)| var.to_string());
				let fontsize = scope
					.get_variable(&VariableName(format!("typst.{}.fontsize", self.env)))
					.map_or("12pt".to_string(), |(var, _)| var.to_string());
				let preamble = scope
					.get_variable(&VariableName(format!("typst.{}.preamble", self.env)))
					.map_or("".to_string(), |(var, _)| var.to_string());
				let prepend = if self.mathmode {
					"".to_string()
				} else {
					scope
						.get_variable(&VariableName(format!("typst.{}.block_prepend", self.env)))
						.map_or("".to_string(), |(var, _)| var.to_string() + "\n")
				};

				let typst = if self.mathmode {
					format_typst(&fontsize, &preamble, &format!("${}$", self.typ))
				} else {
					format_typst(&fontsize, &preamble, &format!("{prepend}{}", self.typ))
				};

				let sanitizer = compiler.sanitizer();
				let location = self.location().clone();
				let caption = self.caption.clone();
				let cache = compiler.get_cache();
				let fut = async move {
					let con = cache.get_connection().await;
					let mut result = match typst.cached(&con, |s| typst_to_svg(s, &exec))
					{
						Ok(s) => s,
						Err(CachedError::SqlErr(e)) => {
							return Err(compile_err!(
								location,
								"Failed to process Typst element".to_string(),
								format!("Querying the cache failed: {e}")
							))
						}
						Err(CachedError::GenErr(e)) => {
							return Err(compile_err!(
								location,
								"Failed to process Typst element".to_string(),
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
				output.add_task(self.location.clone(), "Typst".into(), Box::pin(fut));
			}
			_ => todo!(),
		}
		Ok(())
	}

	fn provide_hover(&self) -> Option<String> {
		Some(format!(
			"Typst

# Properties
 * **Location**: [{}] ({}..{})
 * **Kind**: {}
 * **Mathmode**: {}
 * **Environment**: {}
 * **Caption**: {}",
			self.location.source().name().display(),
			self.location().range.start(),
			self.location().range.end(),
			self.kind,
			self.mathmode,
			self.env,
			self.caption.as_ref().unwrap_or(&"*<none>*".to_string())
		))
	}

	fn lua_ud(&self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(TypstProxy(self as *const _)).unwrap()
	}

	fn lua_ud_mut(&mut self, lua: &Lua) -> AnyUserData {
		lua.create_userdata(TypstProxyMut(self as *mut _)).unwrap()
	}
}
