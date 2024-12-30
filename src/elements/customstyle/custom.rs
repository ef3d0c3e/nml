use ariadne::Fmt;
use lua::kernel::KernelContext;
use std::cell::Ref;
use std::collections::HashMap;
use std::rc::Rc;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;

use crate::document::document::Document;
use crate::lua::kernel::Kernel;
use crate::parser::parser::ParserState;
use crate::parser::reports::Report;
use crate::parser::source::Token;

#[derive(Debug, PartialEq, Eq)]
pub enum CustomStyleToken {
	Toggle(String),
	Pair(String, String),
}

pub trait CustomStyle: core::fmt::Debug {
	/// Name for the custom style
	fn name(&self) -> &str;
	/// Gets the begin and end token for a custom style
	fn tokens(&self) -> &CustomStyleToken;

	fn on_start<'a>(
		&self,
		location: Token,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
	) -> Vec<Report>;
	fn on_end<'a>(
		&self,
		location: Token,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
	) -> Vec<Report>;
}

#[derive(Default)]
pub struct CustomStyleHolder {
	custom_styles: HashMap<String, Rc<dyn CustomStyle>>,
}

impl CustomStyleHolder {
	pub fn get(&self, style_name: &str) -> Option<Rc<dyn CustomStyle>> {
		self.custom_styles.get(style_name).cloned()
	}

	pub fn insert(&mut self, style: Rc<dyn CustomStyle>) {
		self.custom_styles.insert(style.name().into(), style);
	}
}

impl std::ops::Deref for CustomStyleHolder {
	type Target = HashMap<String, Rc<dyn CustomStyle>>;

	fn deref(&self) -> &Self::Target { &self.custom_styles }
}

#[derive(Debug)]
pub struct LuaCustomStyle {
	pub(crate) name: String,
	pub(crate) tokens: CustomStyleToken,
	pub(crate) start: mlua::Function<'static>,
	pub(crate) end: mlua::Function<'static>,
}

impl CustomStyle for LuaCustomStyle {
	fn name(&self) -> &str { self.name.as_str() }

	fn tokens(&self) -> &CustomStyleToken { &self.tokens }

	fn on_start<'a>(
		&self,
		location: Token,
		state: &ParserState,
		document: &'a dyn Document<'a>,
	) -> Vec<Report> {
		let kernel: Ref<'_, Kernel> =
			Ref::map(state.shared.kernels.borrow(), |b| b.get("main").unwrap());
		let mut ctx = KernelContext::new(location.clone(), state, document);

		let mut reports = vec![];
		kernel.run_with_context(&mut ctx, |_lua| {
			if let Err(err) = self.start.call::<_, ()>(()) {
				report_err!(
					&mut reports,
					location.source(),
					"Lua execution failed".into(),
					span(location.range.clone(), err.to_string()),
					note(format!(
						"When trying to start custom style {}",
						self.name().fg(state.parser.colors().info)
					))
				);
			}
		});

		reports.extend(ctx.reports);
		reports
	}

	fn on_end<'a>(
		&self,
		location: Token,
		state: &ParserState,
		document: &'a dyn Document<'a>,
	) -> Vec<Report> {
		let kernel: Ref<'_, Kernel> =
			Ref::map(state.shared.kernels.borrow(), |b| b.get("main").unwrap());
		let mut ctx = KernelContext::new(location.clone(), state, document);

		let mut reports = vec![];
		kernel.run_with_context(&mut ctx, |_lua| {
			if let Err(err) = self.end.call::<_, ()>(()) {
				report_err!(
					&mut reports,
					location.source(),
					"Lua execution failed".into(),
					span(location.range.clone(), err.to_string()),
					note(format!(
						"When trying to end custom style {}",
						self.name().fg(state.parser.colors().info)
					))
				);
			}
		});

		reports.extend(ctx.reports);
		reports
	}
}
