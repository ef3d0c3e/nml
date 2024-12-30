use std::collections::HashMap;
use std::sync::Arc;

use crate::document::document::Document;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::property::PropertyParser;
use crate::parser::reports::macros::*;
use crate::parser::reports::Report;
use crate::parser::reports::*;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;
use ariadne::Fmt;
use graphviz_rust::cmd::Layout;
use lsp::semantic::Semantics;
use lua::kernel::CTX;
use mlua::Error::BadArgument;
use mlua::Function;
use mlua::Lua;
use parser::property::Property;
use parser::util::escape_source;
use parser::util::escape_text;
use regex::Captures;
use regex::Regex;

use super::elem::Graphviz;

fn layout_from_str(value: &str) -> Result<Layout, String> {
	match value {
		"dot" => Ok(Layout::Dot),
		"neato" => Ok(Layout::Neato),
		"fdp" => Ok(Layout::Fdp),
		"sfdp" => Ok(Layout::Sfdp),
		"circo" => Ok(Layout::Circo),
		"twopi" => Ok(Layout::Twopi),
		"osage" => Ok(Layout::Asage), // typo  in graphviz_rust ?
		"patchwork" => Ok(Layout::Patchwork),
		_ => Err(format!("Unknown layout: {value}")),
	}
}

#[auto_registry::auto_registry(registry = "rules")]
pub struct GraphRule {
	re: [Regex; 1],
	properties: PropertyParser,
}

impl Default for GraphRule {
	fn default() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"layout".to_string(),
			Property::new(
				"Graphviz layout engine see <https://graphviz.org/docs/layouts/>".to_string(),
				Some("dot".to_string()),
			),
		);
		props.insert(
			"width".to_string(),
			Property::new("SVG width".to_string(), Some("100%".to_string())),
		);
		Self {
			re: [Regex::new(
				r"\[graph\](?:\[((?:\\.|[^\[\]\\])*?)\])?(?:((?:\\.|[^\\\\])*?)\[/graph\])?",
			)
			.unwrap()],
			properties: PropertyParser { properties: props },
		}
	}
}

impl RegexRule for GraphRule {
	fn name(&self) -> &'static str { "Graphviz" }

	fn previous(&self) -> Option<&'static str> { Some("Tex") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn enabled(&self, mode: &ParseMode, _id: usize) -> bool { !mode.paragraph_only }

	fn on_regex_match(
		&self,
		_: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		let graph_content = match matches.get(2) {
			// Unterminated `[graph]`
			None => {
				report_err!(
					&mut reports,
					token.source(),
					"Unterminamted Graph Code".into(),
					span(
						token.range.clone(),
						format!(
							"Missing terminating `{}` after first `{}`",
							"[/graph]".fg(state.parser.colors().info),
							"[graph]".fg(state.parser.colors().info)
						)
					)
				);
				return reports;
			}
			Some(content) => {
				let processed = escape_text('\\', "[/graph]", content.as_str(), true);

				if processed.is_empty() {
					report_err!(
						&mut reports,
						token.source(),
						"Empty Graph Code".into(),
						span(content.range(), "Graph code is empty".into())
					);
					return reports;
				}
				processed
			}
		};

		// Properties
		let prop_source = escape_source(
			token.source(),
			matches.get(1).map_or(0..0, |m| m.range()),
			"Graphviz Properties".into(),
			'\\',
			"]",
		);
		let properties =
			match self
				.properties
				.parse("Graphviz", &mut reports, state, prop_source.into())
			{
				Some(props) => props,
				None => return reports,
			};
		let (graph_layout, graph_width) = match (
			properties.get(&mut reports, "layout", |_, value| {
				layout_from_str(value.value.as_str())
			}),
			properties.get(&mut reports, "width", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}),
		) {
			(Some(graph_layout), Some(graph_width)) => (graph_layout, graph_width),
			_ => return reports,
		};

		state.push(
			document,
			Box::new(Graphviz {
				location: token.clone(),
				dot: graph_content,
				layout: graph_layout,
				width: graph_width,
			}),
		);

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			let range = token.range;
			sems.add(range.start..range.start + 7, tokens.graph_sep);
			if let Some(props) = matches.get(1).map(|m| m.range()) {
				sems.add(props.start - 1..props.start, tokens.graph_props_sep);
				sems.add(props.end..props.end + 1, tokens.graph_props_sep);
			}
			sems.add(matches.get(2).unwrap().range(), tokens.graph_content);
			sems.add(range.end - 8..range.end, tokens.graph_sep);
		}

		reports
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];
		bindings.push((
			"push".to_string(),
			lua.create_function(|_, (layout, width, dot): (String, String, String)| {
				let mut result = Ok(());

				CTX.with_borrow(|ctx| {
					ctx.as_ref().map(|ctx| {
						let layout = match layout_from_str(layout.as_str()) {
							Err(err) => {
								result = Err(BadArgument {
									to: Some("push".to_string()),
									pos: 1,
									name: Some("layout".to_string()),
									cause: Arc::new(mlua::Error::external(format!(
										"Unable to get layout type: {err}"
									))),
								});
								return;
							}
							Ok(layout) => layout,
						};

						ctx.state.push(
							ctx.document,
							Box::new(Graphviz {
								location: ctx.location.clone(),
								dot,
								layout,
								width,
							}),
						);
					})
				});

				result
			})
			.unwrap(),
		));

		bindings
	}
}
