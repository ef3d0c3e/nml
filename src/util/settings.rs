use std::env::current_dir;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HtmlOutput {
	pub language: String,
	pub icon: Option<String>,
	pub css: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProjectOutput {
	#[serde(rename = "html")]
	Html(HtmlOutput),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSettings {
	pub db_path: String,
	pub output_path: String,
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
	pub fn set_root_path(&mut self, path: &String) -> Result<(), String> {
		fn get_path(name: &str, mut base: PathBuf, component: &str) -> Result<String, String> {
			base.push(component);

			base.to_str()
				.ok_or(format!(
					"Invalid unicode in {name} path: {}",
					base.display()
				))
				.map(|val| val.to_string())
		}

		let cwd = current_dir().map_err(|e| format!("Failed to get working directory: {e}"))?;
		let diff = pathdiff::diff_paths(&path, cwd).unwrap_or(PathBuf::from(path));

		self.output_path = get_path("output", diff.clone(), self.output_path.as_str())?;
		self.db_path = get_path("db", diff.clone(), self.db_path.as_str())?;

		match &mut self.output {
			ProjectOutput::Html(html) => {
				if let Some(icon) = &mut html.icon {
					*icon = get_path("html.icon", diff.clone(), icon.as_str())?;
				}

				if let Some(css) = &mut html.css {
					*css = get_path("html.css", diff.clone(), css.as_str())?;
				}
			}
		}

		Ok(())
	}
}
