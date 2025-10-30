use std::env::current_dir;
use std::path::Path;
use std::path::PathBuf;

use graphviz_rust::print;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HtmlOutput {
	pub language: String,
	pub icon: Option<PathBuf>,
	pub css: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProjectOutput {
	#[serde(rename = "html")]
	Html(HtmlOutput),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
	pub db_path: PathBuf,
	pub output_path: PathBuf,
	pub output: ProjectOutput,
}

impl Default for ProjectSettings {
	fn default() -> Self {
		Self {
			db_path: "".into(),
			output_path: "out".into(),
			output: ProjectOutput::Html(HtmlOutput {
				language: "en-us".into(),
				icon: None,
				css: None,
			}),
		}
	}
}

impl ProjectSettings {
	/// Sets the project's root path
	/// - path: The directory containing the project settings file
	pub fn set_root_path(&mut self, path: PathBuf) -> Result<(), String> {
		fn get_path(mut base: PathBuf, component: &Path) -> Result<PathBuf, String> {
			base = base.join(component);
			base = base.canonicalize().map_err(|e| format!("Failed to canonicalize `{}`: {e}", base.display()))?;
			Ok(base)
		}

		let cwd = current_dir().map_err(|e| format!("Failed to get working directory: {e}"))?;
		let diff = pathdiff::diff_paths(&path, &cwd)
			.unwrap_or(PathBuf::from(path.clone()));

		self.output_path = get_path(diff.clone(), &self.output_path)?;
		self.db_path = get_path(diff.clone(), &self.db_path)?;
		let output_buf = PathBuf::from(&self.output_path).canonicalize().map_err(|e| format!("Failed to canonicalize `{}`: {e}", self.output_path.display()))?;

		let diff = pathdiff::diff_paths(&output_buf, &cwd)
			.unwrap_or(PathBuf::from(output_buf.clone()));
		match &mut self.output {
			ProjectOutput::Html(html) => {
				if let Some(icon) = &mut html.icon {
					println!("ICON");
					*icon = get_path(diff.clone(), &icon)?;
				}

				if let Some(css) = &mut html.css {
					println!("CSS");
					*css = get_path(diff.clone(), &css)?;
				}
			}
		}

		Ok(())
	}
}
