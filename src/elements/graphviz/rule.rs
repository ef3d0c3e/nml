use std::collections::HashMap;
use std::str::FromStr;

use ariadne::Fmt;
use regex::Captures;
use regex::Regex;

use crate::elements::graphviz::completion::GraphvizCompletion;
use crate::elements::graphviz::elem::layout_from_str;
use crate::elements::graphviz::elem::Graphviz;
use crate::layout::size::Size;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::rule::RegexRule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::Token;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::parser::util::escape_source;
use crate::parser::util::escape_text;
use crate::report_err;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;

#[auto_registry::auto_registry(registry = "rules")]
pub struct GraphvizRule {
	re: [Regex; 1],
	properties: PropertyParser,
}

impl Default for GraphvizRule {
	fn default() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"layout".to_string(),
			Property::new(
				"Graphviz layout engine, see <https://graphviz.org/docs/layouts/>".to_string(),
				Some("dot".to_string()),
			),
		);
		props.insert(
			"width".to_string(),
			Property::new("Graph display width".to_string(), Some("100%".to_string())),
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

impl RegexRule for GraphvizRule {
	fn name(&self) -> &'static str {
		"Graphviz"
	}

	fn target(&self) -> RuleTarget {
		RuleTarget::Command
	}

	fn regexes(&self) -> &[regex::Regex] {
		&self.re
	}

	fn enabled(
		&self,
		_unit: &TranslationUnit,
		mode: &ParseMode,
		_states: &mut CustomStates,
		_index: usize,
	) -> bool {
		!mode.paragraph_only
	}

	fn on_regex_match<'u>(
		&self,
		_index: usize,
		unit: &mut TranslationUnit,
		token: Token,
		captures: Captures,
	) {
		let graph_content = match captures.get(2) {
			// Unterminated `$`
			None => {
				report_err!(
					unit,
					token.source(),
					"Unterminated Graphviz Code".into(),
					span(
						token.range.clone(),
						format!(
							"Missing terminating `{}` after first `{}`",
							"[/graph]".fg(unit.colors().info),
							"[graph]".fg(unit.colors().info)
						)
					)
				);
				return;
			}
			Some(content) => {
				let processed = escape_text(
					'\\',
					"[/graph]",
					content.as_str().trim_start().trim_end(),
					true,
				);

				if processed.is_empty() {
					report_err!(
						unit,
						token.source(),
						"Empty Graphviz Code".into(),
						span(content.range(), "Graphviz code is empty".into())
					);
				}
				processed
			}
		};

		// Properties
		let prop_source = escape_source(
			token.source(),
			captures.get(1).map_or(0..0, |m| m.range()),
			"Graphviz Properties".into(),
			'\\',
			"]",
		);
		let Some(mut properties) = self.properties.parse(
			"Graphviz",
			unit,
			Token::new(0..prop_source.content().len(), prop_source),
		) else {
			return;
		};
		let Some(layout) = properties.get(unit, "layout", |_prop, value| {
			layout_from_str(value.value.as_str())
		}) else {
			return;
		};
		let Some(width) = properties.get(unit, "width", |_prop, value| {
			Size::try_from(value.value.as_str())
		}) else {
			return;
		};

		unit.with_lsp(|lsp| {
			lsp.with_semantics(token.source(), |sems, tokens| {
				let range = &token.range;
				sems.add(
					range.start..range.start + "[graph]".len(),
					tokens.graphviz_sep,
				);
				if let Some(props) = captures.get(1).map(|m| m.range()) {
					sems.add(props.start - 1..props.start, tokens.graphviz_prop_sep);
					sems.add(props.end..props.end + 1, tokens.graphviz_prop_sep);
				}
				sems.add(captures.get(2).unwrap().range(), tokens.graphviz_content);
				sems.add(range.end - "[/graph]".len()..range.end, tokens.graphviz_sep);
			})
		});

		unit.add_content(Graphviz {
			location: token,
			graph: graph_content,
			width,
			layout,
		});
	}

	fn completion(
		&self,
	) -> Option<Box<dyn lsp::completion::CompletionProvider + 'static + Send + Sync>> {
		Some(Box::new(GraphvizCompletion {}))
	}
}
