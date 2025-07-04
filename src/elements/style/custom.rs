use std::sync::Arc;

use regex::Regex;

use crate::compiler::compiler::Target;
use crate::unit::translation::CustomData;

use super::state::Style;

pub static STYLE_CUSTOM: &str = "nml.style.registered";

/// Data for styles
pub struct StyleData {
	/// All registered styles
	pub(crate) registered: Vec<Arc<Style>>,
}

impl Default for StyleData {
	fn default() -> Self {
		Self {
			registered: vec![
				Arc::new(Style {
					name: "bold".into(),
					start_re: Regex::new(r"\*\*").unwrap(),
					end_re: Regex::new(r"\*\*").unwrap(),
					compile: Arc::new(|enable, _, compiler, output| {
						output.add_content(match compiler.target() {
							Target::HTML => enable.then_some("<b>").unwrap_or("</b>"),
							_ => todo!(),
						});
						Ok(())
					}),
				}),
				Arc::new(Style {
					name: "italic".into(),
					start_re: Regex::new(r"\*").unwrap(),
					end_re: Regex::new(r"\*").unwrap(),
					compile: Arc::new(|enable, _, compiler, output| {
						output.add_content(match compiler.target() {
							Target::HTML => enable.then_some("<i>").unwrap_or("</i>"),
							_ => todo!(),
						});
						Ok(())
					}),
				}),
				Arc::new(Style {
					name: "underline".into(),
					start_re: Regex::new(r"__").unwrap(),
					end_re: Regex::new(r"__").unwrap(),
					compile: Arc::new(|enable, _, compiler, output| {
						output.add_content(match compiler.target() {
							Target::HTML => enable.then_some("<u>").unwrap_or("</u>"),
							_ => todo!(),
						});
						Ok(())
					}),
				}),
				Arc::new(Style {
					name: "marked".into(),
					start_re: Regex::new(r"`").unwrap(),
					end_re: Regex::new(r"`").unwrap(),
					compile: Arc::new(|enable, _, compiler, output| {
						output.add_content(match compiler.target() {
							Target::HTML => enable.then_some("<em>").unwrap_or("</em>"),
							_ => todo!(),
						});
						Ok(())
					}),
				}),
			],
		}
	}
}

impl CustomData for StyleData {
	fn name(&self) -> &str {
		STYLE_CUSTOM
	}
}
