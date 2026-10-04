//! A workspace served from memory, for a live preview: pages are compiled with the editor's
//! unsaved buffers and linked against the index, and nothing is written.

use std::collections::HashMap;
use std::fs;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use nml_document::ElementKind;
use nml_lang::Language;
use nml_lang::Resolver;
use nml_lang::output_path;
use nml_lua::PageInfo;
use nml_render::Job;
use nml_render::RenderError;
use nml_render::recolor;
use nml_workspace::Config;
use nml_workspace::Index;
use nml_workspace::Role;

use crate::BuildError;
use crate::PageLink;
use crate::WorkspaceResolver;
use crate::language;
use crate::page_options;
use crate::pages;
use crate::read;
use crate::render_into_cache;
use crate::render_job;
use crate::source;
use crate::store_template;
use crate::title;

/// A workspace as a preview serves it.
pub struct Preview {
	root: PathBuf,
	config: Config,
	index: Index,
	language: Language,
	/// Unsaved contents, by workspace path.
	overlay: HashMap<String, String>,
	/// The workspace files pages shown last show, by output path: an unsaved page may show
	/// one the index does not know yet.
	live_assets: HashMap<String, String>,
}

impl std::fmt::Debug for Preview {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Preview")
			.field("root", &self.root)
			.finish_non_exhaustive()
	}
}

/// What is served at a URL.
#[derive(Debug, PartialEq, Eq)]
pub enum Served {
	/// The page of the workspace file at this path: see [`Preview::page`].
	Page(String),
	File {
		content_type: &'static str,
		body: Vec<u8>,
	},
}

/// A page rendered for the preview.
#[derive(Debug)]
pub struct PreviewPage {
	pub html: String,
	/// The renders (math, graphs, …) not in the cache: the page shows their source until
	/// they are rendered with [`Preview::render`].
	pub pending: Vec<PendingRender>,
}

/// A render the cache does not have.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PendingRender {
	/// Its cache key.
	pub key: String,
	job: Job,
}

impl Preview {
	/// Opens the workspace at `root`. Its configuration must be valid.
	pub fn open(root: &Path) -> Result<Self, BuildError> {
		let root = fs::canonicalize(root).map_err(|e| BuildError::Io(root.into(), e))?;
		let config = crate::config(&root)?;
		let index = Index::open(&root.join(&config.database))?;
		Ok(Self {
			root,
			config,
			index,
			language: language(),
			overlay: HashMap::new(),
			live_assets: HashMap::new(),
		})
	}

	pub fn root(&self) -> &Path {
		&self.root
	}

	/// Reads the configuration again, after `nml.toml` was saved. An invalid one is not
	/// used: the build reports it.
	pub fn reload_config(&mut self) -> Result<(), BuildError> {
		let config = crate::config(&self.root)?;
		if config.database != self.config.database {
			self.index = Index::open(&self.root.join(&config.database))?;
		}
		self.config = config;
		Ok(())
	}

	/// Sets the unsaved contents of files, by workspace path.
	pub fn set_overlay(&mut self, overlay: HashMap<String, String>) {
		self.overlay = overlay;
	}

	/// The output path of the page at the workspace path `path`, if it is a page the index
	/// knows.
	pub fn output_of(&self, path: &str) -> Result<Option<String>, BuildError> {
		let unit = self.index.unit(path)?;
		Ok(unit.filter(|u| u.role == Role::Page).map(|u| u.output))
	}

	/// What is at `url`, an output path such as `docs/guide.html`, already percent-decoded.
	/// A directory stands for its `index.html`.
	pub fn lookup(&self, url: &str) -> Result<Option<Served>, BuildError> {
		let mut url = url.trim_start_matches('/').to_string();
		if url.is_empty() || url.ends_with('/') {
			url.push_str("index.html");
		}
		// Only paths inside the output directory.
		let relative = Path::new(&url);
		if !relative
			.components()
			.all(|c| matches!(c, Component::Normal(_)))
		{
			return Ok(None);
		}
		let html = &self.config.html;
		if url == nml_html::DEFAULT_STYLESHEET && html.default_style {
			return Ok(Some(file(&url, nml_html::DEFAULT_STYLE.into())));
		}
		if url == nml_html::CODE_STYLESHEET {
			let css =
				nml_render::stylesheet(&html.code_theme, &html.code_theme_dark).unwrap_or_default();
			return Ok(Some(file(&url, css.into_bytes())));
		}
		for unit in self.index.units()? {
			if unit.role == Role::Page && unit.output == url {
				return Ok(Some(Served::Page(unit.path)));
			}
		}
		let asset = match self.live_assets.get(&url) {
			Some(path) => Some(path.clone()),
			None => (self.index.assets()?.into_iter())
				.find(|asset| asset.output == url)
				.map(|asset| asset.path),
		};
		if let Some(path) = asset {
			return Ok(fs::read(self.root.join(path))
				.ok()
				.map(|body| file(&url, body)));
		}
		if let Some(dir) = &html.static_dir {
			let path = self.root.join(dir).join(relative);
			if path.is_file() {
				return Ok(fs::read(path).ok().map(|body| file(&url, body)));
			}
		}
		Ok(None)
	}

	/// Renders the page at the workspace path `path` as it is now: its file, or its unsaved
	/// content, compiled and linked against the index. References into the page itself see
	/// its unsaved content too; the rest of the workspace is as it was last built. Top-level
	/// blocks carry their source offsets. `None` if the index knows no such page.
	///
	/// A page with errors is still shown, as far as it parses.
	pub fn page(&mut self, path: &str) -> Result<Option<PreviewPage>, BuildError> {
		let Some(unit) = self.index.unit(path)?.filter(|u| u.role == Role::Page) else {
			return Ok(None);
		};
		let absolute = self.root.join(path);
		let source = match self.overlay.get(path) {
			Some(content) => source(&absolute, content.clone()),
			None => read(&absolute)?,
		};
		let resolver: Arc<dyn Resolver> = Arc::new(
			WorkspaceResolver::new(self.root.clone(), self.config.modules.clone())
				.with_overlay(self.overlay.clone()),
		);
		let compiled = nml_lang::compile(&self.language, source.clone(), path, resolver.clone());
		let document = &compiled.unit.document;

		let renderers = &self.config.render;
		let mut rendered = HashMap::new();
		let mut pending = Vec::new();
		for (id, element) in document.descendants(document.root()) {
			let ElementKind::Rendered {
				kind,
				source,
				block,
			} = &element.kind
			else {
				continue;
			};
			// One that cannot render shows its source; the editor reports why.
			let Ok((job, key)) = render_job(renderers, kind, source, *block) else {
				continue;
			};
			match self.index.render(&key)? {
				Some(svg) => {
					rendered.insert(id, recolor(&svg));
				}
				None => pending.push(PendingRender { key, job }),
			}
		}
		pending.sort_by(|a, b| a.key.cmp(&b.key));
		pending.dedup();

		let output = match &compiled.unit.output {
			Some(token) => token.content().to_string(),
			None => output_path(path),
		};
		let basename = path.rsplit('/').next().unwrap();
		let basename = basename.strip_suffix(".nml").unwrap_or(basename);
		let title = title(document).unwrap_or_else(|| basename.to_string());
		let mut options = page_options(&self.config, &output, document, &title, rendered);
		options.source_offsets = true;
		let file_of = |url: &nml_source::Url| crate::workspace_path(&self.root, url);
		let template = store_template(document, &nml_html::compile(document, &options), file_of);

		for asset in &compiled.unit.assets {
			self.live_assets
				.insert(asset.output.clone(), asset.path.clone());
		}
		let pages: Arc<Vec<PageInfo>> = Arc::new(pages(&self.index)?);
		let link = PageLink {
			index: &self.index,
			resolver: &resolver,
			pages: &pages,
			unit: &unit,
			live: Some(&compiled.unit.exports),
		};
		let mut html = Vec::new();
		link.fill(&source, &template, &mut html, &absolute)?;
		let html = String::from_utf8(html).expect("pages are UTF-8");
		Ok(Some(PreviewPage { html, pending }))
	}

	/// Renders into the cache, with the external program. The next [`Preview::page`] shows
	/// it.
	pub fn render(&self, pending: &PendingRender) -> Result<Result<(), RenderError>, BuildError> {
		let rendered =
			render_into_cache(&self.index, &self.config.render, &pending.key, &pending.job)?;
		Ok(rendered.map(|_| ()))
	}
}

fn file(url: &str, body: Vec<u8>) -> Served {
	Served::File {
		content_type: content_type(url),
		body,
	}
}

/// The media type of a file, by its extension.
pub fn content_type(path: &str) -> &'static str {
	let extension = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
	match extension.as_deref() {
		Some("html" | "htm") => "text/html; charset=utf-8",
		Some("css") => "text/css; charset=utf-8",
		Some("js" | "mjs") => "text/javascript; charset=utf-8",
		Some("json") => "application/json",
		Some("txt" | "md" | "nml") => "text/plain; charset=utf-8",
		Some("svg") => "image/svg+xml",
		Some("png") => "image/png",
		Some("jpg" | "jpeg") => "image/jpeg",
		Some("gif") => "image/gif",
		Some("webp") => "image/webp",
		Some("avif") => "image/avif",
		Some("ico") => "image/x-icon",
		Some("mp4") => "video/mp4",
		Some("webm") => "video/webm",
		Some("ogv") => "video/ogg",
		Some("mp3") => "audio/mpeg",
		Some("ogg" | "oga") => "audio/ogg",
		Some("wav") => "audio/wav",
		Some("flac") => "audio/flac",
		Some("woff2") => "font/woff2",
		Some("woff") => "font/woff",
		Some("ttf") => "font/ttf",
		Some("pdf") => "application/pdf",
		_ => "application/octet-stream",
	}
}
