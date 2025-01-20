use std::future::Future;
use std::sync::Once;

use crypto::digest::Digest;
use crypto::sha2::Sha512;
use graphviz_rust::cmd::Format;
use graphviz_rust::cmd::Layout;
use graphviz_rust::exec_dot;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::cache::cache::Cached;
use crate::cache::cache::CachedError;
use crate::compile_err;
use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::CompilerOutput;
use crate::compiler::compiler::Target::HTML;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::parser::reports::Report;
use crate::parser::source::Token;

#[derive(Debug)]
pub struct Graphviz {
	pub(crate) location: Token,
	pub(crate) dot: String,
	pub(crate) layout: Layout,
	pub(crate) width: String,
}

impl Graphviz {
	/// Renders dot to svg
	fn dot_to_svg(&self) -> Result<String, String> {
		print!("Rendering Graphviz `{}`... ", self.dot);

		let svg = match exec_dot(
			self.dot.clone(),
			vec![self.layout.into(), Format::Svg.into()],
		) {
			Ok(svg) => {
				let out = String::from_utf8_lossy(svg.as_slice());
				let svg_start = out.find("<svg").unwrap(); // Remove svg header
				let split_at = out.split_at(svg_start).1.find('\n').unwrap();

				let mut result = format!("<svg width=\"{}\"", self.width);
				result.push_str(out.split_at(svg_start + split_at).1);

				result
			}
			Err(e) => return Err(format!("Unable to execute dot: {e}")),
		};
		println!("Done!");

		Ok(svg)
	}
}

impl Cached for Graphviz {
	type Key = String;
	type Value = String;

	fn sql_table() -> &'static str {
		"CREATE TABLE IF NOT EXISTS cached_dot (
				digest TEXT PRIMARY KEY,
				svg    BLOB NOT NULL);"
	}

	fn sql_get_query() -> &'static str { "SELECT svg FROM cached_dot WHERE digest = (?1)" }

	fn sql_insert_query() -> &'static str { "INSERT INTO cached_dot (digest, svg) VALUES (?1, ?2)" }

	fn key(&self) -> <Self as Cached>::Key {
		let mut hasher = Sha512::new();
		hasher.input((self.layout as usize).to_be_bytes().as_slice());
		hasher.input(self.width.as_bytes());
		hasher.input(self.dot.as_bytes());

		hasher.result_str()
	}
}

impl Element for Graphviz {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Block }

	fn element_name(&self) -> &'static str { "Graphviz" }

	fn compile<'e>(
		&'e self,
		compiler: &'e Compiler,
		_document: &dyn Document,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			HTML => {
				static CACHE_INIT: Once = Once::new();
				CACHE_INIT.call_once(|| {
					if let Some(con) = compiler.cache() {
						if let Err(e) = Graphviz::init(con) {
							eprintln!("Unable to create cache table: {e}");
						}
					}
				});

				// TODO: Format svg in a div
				/*
				let fut = async move {
					if let Some(con) = compiler.cache() {
						match self.cached(con, |s| s.dot_to_svg()) {
							Ok(s) => Ok(s),
							Err(e) => match e {
								CachedError::SqlErr(e) =>
									Err(compile_err!(
											self.location(),
											"Failed to compile Graphviz element".into(),
											format!("Querying the cache failed: {e}")
									)),
								CachedError::GenErr(e) =>
									Err(compile_err!(
											self.location(),
											"Failed to compile Graphviz element".into(),
											e.to_string()
									)),
							},
						}
					} else {
						match self.dot_to_svg() {
							Ok(svg) => Ok(svg),
							Err(e) => Err(compile_err!(
									self.location(),
									"Failed to compile Graphviz element".into(),
									e.to_string()
							)),
						}
					}
				};
				output.add_task(Box::pin(fut));
				*/
			}
			_ => todo!("Unimplemented"),
		}
		Ok(())
	}
}

unsafe impl Sync for Graphviz {}
