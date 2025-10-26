use std::sync::Arc;

use ariadne::Fmt;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::source::Token;
use crate::unit::translation::CustomData;
use crate::unit::translation::TranslationUnit;

use super::state::Layout;

pub static LAYOUT_CUSTOM: &str = "nml.layout.registered";

/// Data for layouts
pub struct LayoutData {
	/// All registered layouts
	pub(crate) registered: Vec<Arc<dyn Layout + Send + Sync>>,
}

impl Default for LayoutData {
	fn default() -> Self {
		Self { registered: vec![
			Arc::new(CenterLayout{})
		] }
	}
}

impl CustomData for LayoutData {
	fn name(&self) -> &str {
		LAYOUT_CUSTOM
	}
}

#[derive(Debug)]
pub struct CenterLayout;

impl Layout for CenterLayout {
	fn name(&self) -> &str {
		"center"
	}

	fn expects(&self) -> std::ops::Range<usize> {
		1..1
	}

	fn parse_properties(
		&self,
		unit: &mut TranslationUnit,
		token: Token,
	) -> Option<Box<dyn std::any::Any>> {
		if token.end() != token.start() {
			report_err!(
				unit,
				token.source(),
				"Invalid Properties for Layout".into(),
				span(
					token.range.clone(),
					format!(
						"Layout {} expects no properties",
						self.name().fg(unit.colors().info)
					)
				),
			);
			return None;
		}
		Some(Box::new([0; 0]))
	}
}
