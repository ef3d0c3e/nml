use mlua::Function;
use mlua::Lua;
use regex::Captures;
use regex::Regex;
use regex::RegexBuilder;

use crate::document::document::Document;
use crate::elements::toc::elem::Toc;
use crate::lsp::semantic::Semantics;
use crate::lua::kernel::CTX;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::reports::Report;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;

#[auto_registry::auto_registry(registry = "rules")]
pub struct TocRule {
	re: [Regex; 1],
}

impl Default for TocRule {
	fn default() -> Self {
		Self {
			re: [
				RegexBuilder::new(r"(?:^|\n)(?:[^\S\n]*)#\+TABLE_OF_CONTENT(.*)")
					.multi_line(true)
					.build()
					.unwrap(),
			],
		}
	}
}

impl RegexRule for TocRule {
	fn name(&self) -> &'static str { "Toc" }

	fn previous(&self) -> Option<&'static str> { Some("Layout") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn enabled(&self, mode: &ParseMode, _id: usize) -> bool { !mode.paragraph_only }

	fn on_regex_match(
		&self,
		_index: usize,
		state: &ParserState,
		document: &dyn Document,
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let name = matches.get(1).unwrap().as_str().trim_start().trim_end();

		state.push(
			document,
			Box::new(Toc {
				location: token.clone(),
				title: (!name.is_empty()).then_some(name.to_string()),
			}),
		);

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			let start = matches
				.get(0)
				.map(|m| m.start() + token.source().content()[m.start()..].find('#').unwrap())
				.unwrap();
			sems.add(start..start + 2, tokens.toc_sep);
			sems.add(
				start + 2..start + 2 + "TABLE_OF_CONTENT".len(),
				tokens.toc_token,
			);
			sems.add(matches.get(1).unwrap().range(), tokens.toc_title);
		}

		vec![]
	}

	fn register_bindings<'lua>(&self, lua: &'lua Lua) -> Vec<(String, Function<'lua>)> {
		let mut bindings = vec![];
		bindings.push((
			"push".to_string(),
			lua.create_function(|_, title: Option<String>| {
				CTX.with_borrow(|ctx| {
					if let Some(ctx) = ctx.as_ref() { ctx.state.push(
							ctx.document,
							Box::new(Toc {
								location: ctx.location.clone(),
								title,
							}),
						) }
				});
				Ok(())
			})
			.unwrap(),
		));
		bindings
	}
}
