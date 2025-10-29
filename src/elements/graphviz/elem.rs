use std::sync::Arc;
use std::sync::Once;

use ariadne::Span;
use auto_userdata::AutoUserData;
use crypto::digest::Digest;
use crypto::sha2::Sha512;
use graphviz_rust::cmd::Format;
use graphviz_rust::cmd::Layout;
use graphviz_rust::exec_dot;
use mlua::AnyUserData;
use mlua::Lua;
use parking_lot::RwLock;

use crate::layout::size::Size;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use mlua::LuaSerdeExt;

use crate::cache::cache::Cached;
use crate::cache::cache::CachedError;
use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::Report;
use crate::parser::source::Token;
use crate::unit::element::ElemKind;
use crate::unit::element::Element;
use crate::unit::scope::Scope;

pub(crate) fn layout_to_str(layout: Layout) -> &'static str {
	match layout {
		Layout::Dot => "Dot",
		Layout::Neato => "Neato",
		Layout::Twopi => "Twopi",
		Layout::Circo => "Circo",
		Layout::Fdp => "Fdp",
		Layout::Asage => "Asage",
		Layout::Patchwork => "Patchwork",
		Layout::Sfdp => "Sfdp",
	}
}

pub(crate) fn layout_from_str(value: &str) -> Result<Layout, String> {
	match value {
		"dot" => Ok(Layout::Dot),
		"neato" => Ok(Layout::Neato),
		"fdp" => Ok(Layout::Fdp),
		"sfdp" => Ok(Layout::Sfdp),
		"circo" => Ok(Layout::Circo),
		"twopi" => Ok(Layout::Twopi),
		"osage" => Ok(Layout::Asage), // typo  in graphviz_rust ?
		"patchwork" => Ok(Layout::Patchwork),
		_ => Err(format!("Unknown layout: {value}")),
	}
}

#[derive(Debug, Clone, AutoUserData)]
#[auto_userdata_target = "&"]
#[auto_userdata_target = "*"]
pub struct Graphviz {
	pub(crate) location: Token,
	pub(crate) graph: String,
	#[lua_ignore]
	pub(crate) layout: Layout,
	#[lua_value]
	pub(crate) width: Size,
}

impl Graphviz {
	pub fn dot_to_svg(&self) -> Result<String, String> {
		let svg = match exec_dot(
			self.graph.clone(),
			vec![self.layout.into(), Format::Svg.into()],
		) {
			Ok(svg) => {
				let out = String::from_utf8_lossy(svg.as_slice());
				let svg_start = out.find("<svg").unwrap(); // Remove svg header
				let split_at = out.split_at(svg_start).1.find('\n').unwrap();

				let mut result = format!("<svg width=\"{}\"", self.width.to_output(Target::HTML));
				result.push_str(out.split_at(svg_start + split_at).1);

				result
			}
			Err(e) => return Err(format!("Unable to execute dot: {e}")),
		};
		Ok(svg)
	}
}

impl Cached for Graphviz {
	type Key = String;
	type Value = String;

	fn sql_table() -> &'static str {
		"CREATE TABLE IF NOT EXISTS cached_graphviz (
				digest TEXT PRIMARY KEY,
				svg    BLOB NOT NULL);"
	}

	fn sql_get_query() -> &'static str {
		"SELECT svg FROM cached_graphviz WHERE digest = (?1)"
	}

	fn sql_insert_query() -> &'static str {
		"INSERT INTO cached_graphviz (digest, svg) VALUES (?1, ?2)"
	}

	fn key(&self) -> <Self as Cached>::Key {
		let mut hasher = Sha512::new();
		hasher.input((self.layout as usize).to_be_bytes().as_slice());
		hasher.input(self.width.to_string().as_bytes());
		hasher.input(self.graph.as_bytes());
		hasher.result_str()
	}
}

impl Element for Graphviz {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Block
	}

	fn element_name(&self) -> &'static str {
		"Graphviz"
	}

	fn compile<'e>(
		&'e self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &'e Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			Target::HTML => {
				static CACHE_INIT: Once = Once::new();

				CACHE_INIT.call_once(|| {
					let cache = compiler.get_cache();
					let con = tokio::runtime::Runtime::new()
						.unwrap()
						.block_on(cache.get_connection());
					if let Err(e) = Graphviz::init(&con) {
						eprintln!("Unable to create cache table: {e}");
					}
				});

				let value = self.clone();

				let cache = compiler.get_cache();
				let fut = async move {
					let con = cache.get_connection().await;
					let result = match value.cached(&con, |s| s.dot_to_svg()) {
						Ok(s) => s,
						Err(CachedError::SqlErr(e)) => {
							return Err(compile_err!(
								value.location,
								"Failed to process Graphviz element".to_string(),
								format!("Querying the cache failed: {e}")
							))
						}
						Err(CachedError::GenErr(e)) => {
							return Err(compile_err!(
								value.location,
								"Failed to process Graphviz element".to_string(),
								e
							))
						}
					};
					Ok(result)
				};
				output.add_task(self.location.clone(), "Graphviz".into(), Box::pin(fut));
			}
			_ => todo!(),
		}
		Ok(())
	}

	fn provide_hover(&self) -> Option<String> {
		Some(format!(
			"Graphviz

# Properties
 * **Location**: [{}] ({}..{})
 * **Layout**: {}
 * **Width**: {}",
			self.location.source().name().display(),
			self.location().range.start(),
			self.location().range.end(),
			layout_to_str(self.layout),
			self.width.to_string()
		))
	}

	fn lua_wrap(self: Arc<Self>, lua: &Lua) -> Option<AnyUserData> {
		let r: &'static _ = unsafe { &*Arc::as_ptr(&self) };
		Some(lua.create_userdata(r).unwrap())
	}
}
