use std::sync::Arc;
use std::sync::OnceLock;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::compiler::output::CompilerOutput;
use crate::layout::size::SizeOutput;
use crate::lua::wrappers::*;
use crate::parser::reports::Report;
use crate::unit::element::ElemKind;
use crate::unit::element::ReferenceableElement;
use crate::unit::references::InternalReference;
use crate::unit::scope::Scope;
use crate::unit::scope::ScopeAccessor;
use auto_userdata::AutoUserData;
use mlua::AnyUserData;
use mlua::Lua;
use parking_lot::RwLock;
use serde::Deserialize;
use serde::Serialize;
use url::Url;

use crate::layout::size::Size;
use crate::parser::source::Token;
use crate::unit::element::Element;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MediaType {
	Image,
	Video,
	Audio,
}

impl MediaType {
	pub fn from_filename(name: &str) -> Option<MediaType> {
		let pos = name.rfind('.')?;

		// TODO: https://developer.mozilla.org/en-US/docs/Web/Media/Formats/Containers
		match name.split_at(pos + 1).1.to_ascii_lowercase().as_str() {
			"png" | "apng" | "avif" | "gif" | "webp" | "svg" | "bmp" | "jpg" | "jpeg" | "jfif"
			| "pjpeg" | "pjp" => Some(MediaType::Image),
			"mp4" | "m4v" | "webm" | "mov" => Some(MediaType::Video),
			"mp3" | "ogg" | "flac" | "wav" => Some(MediaType::Audio),
			_ => None,
		}
	}
}

impl TryFrom<&str> for MediaType {
	type Error = String;

	fn try_from(value: &str) -> Result<Self, Self::Error> {
		match value {
			"image" => Ok(MediaType::Image),
			"video" => Ok(MediaType::Video),
			"audio" => Ok(MediaType::Audio),
			_ => Err(format!("Unknown media type: {value}")),
		}
	}
}

#[derive(Debug, AutoUserData)]
#[auto_userdata_target = "*"]
#[auto_userdata_target = "&"]
#[auto_userdata_target = "&mut"]
pub struct MediaGroup {
	pub(crate) location: Token,
	#[lua_ignore]
	pub(crate) media: Vec<Arc<Media>>,
}

impl MediaGroup {
	pub fn add_media(&mut self, media: Arc<Media>)
	{
		self.location.range.end = media.location().end();
		self.media.push(media);
	}
}

impl Element for MediaGroup {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> ElemKind {
		ElemKind::Block
	}

	fn element_name(&self) -> &'static str {
		"MediaGroup"
	}

	fn compile(
		&self,
		scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			Target::HTML => {
				output.add_content("<div class=\"media\">");
				for media in self.media.iter() {
					media.compile(scope.clone(), compiler, output)?;
				}
				output.add_content("</div>");
			},
			_ => todo!(),
		}
		Ok(())
	}

	fn lua_wrap(self: Arc<Self>, lua: &Lua) -> Option<AnyUserData> {
		let r: &'static _ = unsafe { &*Arc::as_ptr(&self) };
		Some(lua.create_userdata(r).unwrap())
	}
}

#[derive(Debug, AutoUserData)]
#[auto_userdata_target = "*"]
#[auto_userdata_target = "&"]
#[auto_userdata_target = "&mut"]
pub struct Media {
	pub(crate) location: Token,
	#[lua_ignore]
	pub(crate) url: Url,
	#[lua_value]
	pub(crate) media_type: MediaType,
	#[lua_value]
	pub(crate) width: Option<Size>,
	#[lua_value]
	pub(crate) caption: Option<String>,
	#[lua_ignore]
	pub(crate) description: Option<Arc<RwLock<Scope>>>,
	#[lua_ignore]
	pub(crate) reference: Option<Arc<InternalReference>>,
	#[lua_map(OnceLockWrapper)]
	pub(crate) link: OnceLock<String>,
}

impl Element for Media {
	fn location(&self) -> &Token {
		&self.location
	}

	fn kind(&self) -> crate::unit::element::ElemKind {
		ElemKind::Block
	}

	fn element_name(&self) -> &'static str {
		"media"
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			Target::HTML => {
				let id = output.refid(self);
				let width = self
					.width
					.map_or(String::default(), |w| w.to_output(SizeOutput::CSS));

				output.add_content(r#"<div class="media">"#);
				output.add_content(match self.media_type {
					MediaType::Image => format!(
						r#"<a href="{0}"><img src="{0}"{width}></a>"#,
						compiler.sanitize(self.url.to_string())
					),
					MediaType::Video => format!(
						r#"<video controls {width}><source src="{0}"></video>"#,
						compiler.sanitize(self.url.to_string())
					),
					MediaType::Audio => format!(
						r#"<audio controls src="{0}"{width}></audio>"#,
						compiler.sanitize(self.url.to_string())
					),
				});

				output.add_content(format!(
					r#"<p class="media-refname"><a class="media-refname-id">({id})</a>{}</p>"#,
					self.caption
						.as_ref()
						.map_or(String::default(), |cap| format!(" {cap}"))
				));

				if let Some(description) = &self.description
				{
					for (scope, elem) in description.content_iter(false) {
						elem.compile(scope, compiler, output)?;
					}
				}
				output.add_content(r#"</div>"#);
			}
			_ => todo!(),
		}
		Ok(())
	}

	fn as_referenceable(self: Arc<Self>) -> Option<Arc<dyn ReferenceableElement>> {
		Some(self)
	}

	fn lua_wrap(self: Arc<Self>, lua: &Lua) -> Option<AnyUserData> {
		let r: &'static _ = unsafe { &*Arc::as_ptr(&self) };
		Some(lua.create_userdata(r).unwrap())
	}
}

impl ReferenceableElement for Media {
	fn reference(&self) -> Arc<InternalReference> {
		self.reference.to_owned().unwrap()
	}

	fn refcount_key(&self) -> &'static str {
		"media"
	}

	fn refid(&self, _compiler: &Compiler, refid: usize) -> String {
		refid.to_string()
	}

	fn get_link(&self) -> Option<&String> {
		self.link.get()
	}

	fn set_link(&self, url: String) {
		self.link.set(url).expect("set_url can only be called once");
	}
}
