use std::env::current_dir;
use std::fmt::format;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::compiler::output;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationUnit;
use crate::unit::variable::VariableName;

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
			db_path: "cache.db".into(),
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
	/// Add configuration details from the translation unit
	pub fn source_unit(&mut self, unit: &TranslationUnit) {
		let scope = unit.get_scope();

		match &mut self.output {
			ProjectOutput::Html(html) => {
				if let Some((var, _)) =
					scope.get_variable(&VariableName("html.language".to_string()))
				{
					html.language = var.to_string();
				}
				if let Some((var, _)) = scope.get_variable(&VariableName("html.icon".to_string())) {
					html.icon = Some(var.to_string())
				}
				if let Some((var, _)) = scope.get_variable(&VariableName("html.css".to_string())) {
					html.icon = Some(var.to_string());
				}
			}
			_ => todo!(),
		}
	}

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
