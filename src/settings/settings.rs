use serde::Deserialize;
use serde::Serialize;

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
	pub output_path: Option<String>,
	pub output: ProjectOutput,
}

impl Default for ProjectSettings {
	fn default() -> Self {
		Self {
			db_path: "cache.db".into(),
			output_path: None,
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
				if let Some((var, _)) =
					scope.get_variable(&VariableName("html.icon".to_string()))
				{
					html.icon = Some(var.to_string());
				}
				if let Some((var, _)) =
					scope.get_variable(&VariableName("html.css".to_string()))
				{
					html.icon = Some(var.to_string());
				}
			}
			_ => todo!(),
		}
	}
}
