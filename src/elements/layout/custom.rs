use std::sync::Arc;

use regex::Regex;

use crate::compiler::compiler::Target;
use crate::unit::translation::CustomData;

use super::state::Layout;

pub static STYLE_CUSTOM: &str = "nml.layout.registered";

/// Data for styles
pub struct LayoutData {
	/// All registered styles
	pub(crate) registered: Vec<Arc<Layout>>,
}

impl Default for LayoutData {
	fn default() -> Self {
		Self {
			registered: vec![
				Arc::new(Layout {
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
				Arc::new(Layout {
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
				Arc::new(Layout {
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
				Arc::new(Layout {
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

impl CustomData for LayoutData {
	fn name(&self) -> &str {
		STYLE_CUSTOM
	}
}
