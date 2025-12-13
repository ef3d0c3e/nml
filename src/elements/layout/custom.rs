use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use ariadne::Fmt;
use parking_lot::RwLock;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::compiler::output::CompilerOutput;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::source::Token;
use crate::unit::scope::Scope;
use crate::unit::translation::CustomData;
use crate::unit::translation::TranslationUnit;

use super::elem::LayoutToken;
use super::state::Layout;

pub static LAYOUT_CUSTOM: &str = "nml.layout.registered";

/// Data for layouts
pub struct LayoutData {
	/// All registered layouts
	pub(crate) registered: HashMap<String, Arc<dyn Layout + Send + Sync>>,
}

impl Default for LayoutData {
	fn default() -> Self {
		let mut map: HashMap<String, Arc<dyn Layout + Send + Sync>> = HashMap::default();
		let center = CenterLayout {};
		map.insert(center.name().to_string(), Arc::new(center));
		let spoiler = SpoilerLayout {};
		map.insert(spoiler.name().to_string(), Arc::new(spoiler));
		Self { registered: map }
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
		id: usize,
	) -> Option<Box<dyn Any + Send + Sync>> {
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

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
		_id: usize,
		token: LayoutToken,
		_params: &Option<Box<dyn Any + Send + Sync>>,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			Target::HTML => {
				if token == LayoutToken::Start {
					output.add_content(r#"<div style="margin: auto">"#);
				} else if token == LayoutToken::End {
					output.add_content("</div>");
				}
			}
			Target::LATEX => todo!(),
		}
		Ok(())
	}
}

#[derive(Debug)]
pub struct SpoilerLayout;

pub struct SpoilerData {
	label: String,
}

impl Layout for SpoilerLayout {
	fn name(&self) -> &str {
		"spoiler"
	}

	fn expects(&self) -> std::ops::Range<usize> {
		1..1
	}

	fn parse_properties(
		&self,
		unit: &mut TranslationUnit,
		token: Token,
		_id: usize,
	) -> Option<Box<dyn Any + Send + Sync>> {
		if token.end() != token.start() {
			let label = token.content().into();
			return Some(Box::new(SpoilerData { label }));
		}
		Some(Box::new(SpoilerData {
			label: "Spoiler".into(),
		}))
	}

	fn compile(
		&self,
		_scope: Arc<RwLock<Scope>>,
		compiler: &Compiler,
		output: &mut CompilerOutput,
		_id: usize,
		token: LayoutToken,
		params: &Option<Box<dyn Any + Send + Sync>>,
	) -> Result<(), Vec<Report>> {
		match compiler.target() {
			Target::HTML => {
				if token == LayoutToken::Start {
					let data = params
						.as_ref()
						.unwrap()
						.downcast_ref::<SpoilerData>()
						.unwrap();
					let id = compiler.random_id();
					output.add_content(r#"<div class="spoiler-container">"#);
					output.add_content(format!(
						r#"<input type="checkbox" id="{id}" class="spoiler-toggle">"#
					));
					output.add_content(format!(
						r#"<label for="{id}" class="spoiler-label">{}</label>"#,
						data.label
					));
					output.add_content(format!(r#"<div class="spoiler-content">"#));
				} else if token == LayoutToken::End {
					output.add_content("</div></div>");
				}
			}
			Target::LATEX => todo!(),
		}
		Ok(())
	}
}
