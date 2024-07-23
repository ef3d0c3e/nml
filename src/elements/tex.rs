use std::{io::{Read, Write}, ops::Range, process::{Command, Stdio}, rc::Rc, sync::Once};

use ariadne::{Fmt, Label, Report, ReportKind};
use crypto::{digest::Digest, sha2::Sha512};
use mlua::{Function, Lua};
use regex::{Captures, Regex};

use crate::{cache::cache::{Cached, CachedError}, compiler::compiler::{Compiler, Target}, document::{document::Document, element::{ElemKind, Element}}, parser::{parser::Parser, rule::RegexRule, source::{Source, Token}, util}};

#[derive(Debug, PartialEq, Eq)]
enum TexKind
{
	Block,
	Inline,
}

impl From<&TexKind> for ElemKind
{
    fn from(value: &TexKind) -> Self {
		match value {
			TexKind::Inline => ElemKind::Inline,
			_ => ElemKind::Block
		}
    }
}

#[derive(Debug)]
struct Tex
{
	location: Token,
	block: TexKind,
	env: String,
	tex: String,
	caption: Option<String>,
}

impl Tex {
    fn new(location: Token, block: TexKind, env: String, tex: String, caption: Option<String>) -> Self {
        Self { location, block, env, tex, caption }
    }

	fn format_latex(fontsize: &String, preamble: &String, tex: &String) -> FormattedTex
	{
		FormattedTex(format!(r"\documentclass[{}pt,preview]{{standalone}}
{}
\begin{{document}}
\begin{{preview}}
{}
\end{{preview}}
\end{{document}}",
		fontsize, preamble, tex))
	}
}

struct FormattedTex(String);

impl FormattedTex
{
	/// Renders latex to svg
	fn latex_to_svg(&self, exec: &String, fontsize: &String) -> Result<String, String>
	{
		print!("Rendering LaTex `{}`... ", self.0);
		let process = match Command::new(exec)
			.arg("--fontsize").arg(fontsize)
			.stdout(Stdio::piped())
			.stdin(Stdio::piped())
			.spawn()
			{
				Err(e) => return Err(format!("Could not spawn `{exec}`: {}", e)),
				Ok(process) => process
			};

		if let Err(e) = process.stdin.unwrap().write_all(self.0.as_bytes())
		{
			panic!("Unable to write to `latex2svg`'s stdin: {}", e);
		}

		let mut result = String::new();
		match process.stdout.unwrap().read_to_string(&mut result)
		{
			Err(e) => panic!("Unable to read `latex2svg` stdout: {}", e),
			Ok(_) => {}
		}
		println!("Done!");

		Ok(result)
	}
}

impl Cached for FormattedTex
{
    type Key = String;
    type Value = String;

    fn sql_table() -> &'static str {
		"CREATE TABLE IF NOT EXISTS cached_tex (
				digest TEXT PRIMARY KEY,
				svg    BLOB NOT NULL);"
    }

    fn sql_get_query() -> &'static str {
		"SELECT svg FROM cached_tex WHERE digest = (?1)"
    }

    fn sql_insert_query() -> &'static str {
		"INSERT INTO cached_tex (digest, svg) VALUES (?1, ?2)"
    }

    fn key(&self) -> <Self as Cached>::Key {
		let mut hasher = Sha512::new();
		hasher.input(self.0.as_bytes());

		hasher.result_str()
    }
}

impl Element for Tex {
    fn location(&self) -> &Token { &self.location }

    fn kind(&self) -> ElemKind { (&self.block).into() }

    fn element_name(&self) -> &'static str { "LaTeX" }

    fn to_string(&self) -> String { format!("{self:#?}") }

    fn compile(&self, compiler: &Compiler, document: &dyn Document)
		-> Result<String, String> {

		match compiler.target() {
			Target::HTML => {
				static CACHE_INIT : Once = Once::new();
				CACHE_INIT.call_once(|| if let Some(mut con) = compiler.cache() {
					if let Err(e) = FormattedTex::init(&mut con)
					{
						eprintln!("Unable to create cache table: {e}");
					}
				});

				let exec = document.get_variable(format!("tex.{}.exec", self.env).as_str())
					.map_or("latex2svg".to_string(), |var| var.to_string());
				// FIXME: Because fontsize is passed as an arg, verify that it cannot be used to execute python/shell code
				let fontsize = document.get_variable(format!("tex.{}.fontsize", self.env).as_str())
					.map_or("12".to_string(), |var| var.to_string());
				let preamble = document.get_variable(format!("tex.{}.preamble", self.env).as_str())
					.map_or("".to_string(), |var| var.to_string());
				let prepend = if self.block == TexKind::Inline { "".to_string() }
				else
				{
					document.get_variable(format!("tex.{}.block_prepend", self.env).as_str())
						.map_or("".to_string(), |var| var.to_string()+"\n")
				};

				let latex = match self.block
				{
					TexKind::Inline => Tex::format_latex(
						&fontsize,
						&preamble,
						&format!("${{{}}}$", self.tex)),
					_ => Tex::format_latex(
						&fontsize,
						&preamble,
						&format!("{prepend}{}", self.tex))
				};

				if let Some(mut con) = compiler.cache()
				{
					match latex.cached(&mut con, |s| s.latex_to_svg(&exec, &fontsize))
					{
						Ok(s) => Ok(s),
						Err(e) => match e
						{
							CachedError::SqlErr(e) => Err(format!("Querying the cache failed: {e}")),
							CachedError::GenErr(e) => Err(e)
						}
					}
				}
				else
				{
					latex.latex_to_svg(&exec, &fontsize)
				}
			}
			_ => todo!("Unimplemented")
		}
    }
}

pub struct TexRule {
	re: [Regex; 2],
}

impl TexRule {
	pub fn new() -> Self {
		Self {
			re: [
				Regex::new(r"\$\|(?:\[(.*)\])?(?:((?:\\.|[^\\\\])*?)\|\$)?").unwrap(),
				Regex::new(r"\$(?:\[(.*)\])?(?:((?:\\.|[^\\\\])*?)\$)?").unwrap(),
			],
		}
	}
}

impl RegexRule for TexRule
{
    fn name(&self) -> &'static str { "Tex" }

    fn regexes(&self) -> &[regex::Regex] { &self.re }

    fn on_regex_match(&self, index: usize, parser: &dyn Parser, document: &dyn Document, token: Token, matches: Captures)
		-> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let tex_env = matches.get(1)
			.and_then(|env| Some(env.as_str().trim_start().trim_end()))
			.and_then(|env| (!env.is_empty()).then_some(env))
			.unwrap_or("main");

		let tex_content = match matches.get(2)
		{
			// Unterminated `$`
			None => {
				reports.push(
					Report::build(ReportKind::Error, token.source(), token.start())
					.with_message("Unterminated Tex Code")
					.with_label(
						Label::new((token.source().clone(), token.range.clone()))
						.with_message(format!("Missing terminating `{}` after first `{}`",
							["|$", "$"][index].fg(parser.colors().info),
							["$|", "$"][index].fg(parser.colors().info)))
						.with_color(parser.colors().error))
					.finish());
				return reports;
			}
			Some(content) => {
				let processed = util::process_escaped('\\', ["|$", "$"][index],
					content.as_str().trim_start().trim_end());

				if processed.is_empty()
				{
					reports.push(
						Report::build(ReportKind::Warning, token.source(), content.start())
						.with_message("Empty Tex Code")
						.with_label(
							Label::new((token.source().clone(), content.range()))
							.with_message("Tex code is empty")
							.with_color(parser.colors().warning))
						.finish());
				}
				processed
			}
		};

		// TODO: Caption

		parser.push(document, Box::new(Tex::new(
			token,
			if index == 1 { TexKind::Inline } else { TexKind::Block },
			tex_env.to_string(),
			tex_content,
			None,
		)));

		reports
    }

	// TODO
	fn lua_bindings<'lua>(&self, _lua: &'lua Lua) -> Vec<(String, Function<'lua>)> { vec![] }
}
