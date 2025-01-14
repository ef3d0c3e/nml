use std::rc::Rc;
use std::sync::Arc;

use crate::elements::text::elem::Text;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use ariadne::Fmt;
use document::document::Document;
use lsp::code::CodeRange;
use lsp::hints::Hints;
use lsp::semantic::Semantics;
use lua::kernel::Kernel;
use lua::kernel::KernelContext;
use mlua::Lua;
use parser::parser::ParseMode;
use parser::parser::ParserState;
use parser::rule::RegexRule;
use parser::source::Source;
use parser::source::Token;
use parser::source::VirtualSource;
use parser::util::escape_source;
use parser::util::process_text;
use regex::Captures;
use regex::Regex;

use crate::parser::parser::ReportColors;

#[auto_registry::auto_registry(registry = "rules")]
pub struct ScriptRule {
	re: [Regex; 2],
	eval_kinds: [(&'static str, &'static str); 3],
}

impl ScriptRule {
	fn validate_kind(&self, colors: &ReportColors, kind: &str) -> Result<usize, String> {
		match self
			.eval_kinds
			.iter()
			.position(|(kind_symbol, _)| kind == *kind_symbol)
		{
			Some(id) => Ok(id),
			None => Err(format!(
				"Unable to find eval kind `{}`. Available kinds:{}",
				kind.fg(colors.highlight),
				self.eval_kinds
					.iter()
					.fold(String::new(), |out, (symbol, name)| {
						out + format!("\n - '{symbol}' => {name}").as_str()
					})
			)),
		}
	}
}

impl Default for ScriptRule {
	fn default() -> Self {
		Self {
			re: [
				Regex::new(r"(?:^|\n)@<(?:(.*)\n?)((?:\\.|[^\\\\])*?)(?:\n?)>@").unwrap(),
				Regex::new(r"%<(?:\[(.*?)\])?([^\s[:alpha:]])?((?:\\.|[^\\\\])*?)(?:\n?)>%")
					.unwrap(),
			],
			eval_kinds: [
				("", "Eval"),
				("\"", "Eval to text"),
				("!", "Eval and parse"),
			],
		}
	}
}

fn validate_kernel_name(colors: &ReportColors, name: &str) -> Result<String, String> {
	let trimmed = name.trim_end().trim_start();
	if trimmed.is_empty() {
		return Ok("main".to_string());
	} else if trimmed.find(|c: char| c.is_whitespace()).is_some() {
		return Err(format!(
			"Kernel name `{}` contains whitespaces",
			trimmed.fg(colors.highlight)
		));
	}

	Ok(trimmed.to_string())
}

impl RegexRule for ScriptRule {
	fn name(&self) -> &'static str { "Script" }

	fn previous(&self) -> Option<&'static str> { Some("Import") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn enabled(&self, mode: &ParseMode, id: usize) -> bool { !mode.paragraph_only || id != 0 }

	fn on_regex_match<'a>(
		&self,
		index: usize,
		state: &ParserState,
		document: &'a dyn Document<'a>,
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		let kernel_name = match matches.get(1) {
			None => "main".to_string(),
			Some(name) => match validate_kernel_name(state.parser.colors(), name.as_str()) {
				Ok(name) => name,
				Err(e) => {
					report_err!(
						&mut reports,
						token.source(),
						"Invalid Kernel Name".into(),
						span(name.range(), e)
					);
					return reports;
				}
			},
		};
		let mut kernels_borrow = state.shared.kernels.borrow_mut();
		let kernel = match kernels_borrow.get(kernel_name.as_str()) {
			Some(kernel) => kernel,
			None => {
				kernels_borrow.insert(kernel_name.clone(), Kernel::new(state.parser));
				kernels_borrow.get(kernel_name.as_str()).unwrap()
			}
		};

		let script_range = matches.get(if index == 0 { 2 } else { 3 }).unwrap().range();
		let source = escape_source(
			token.source(),
			script_range.clone(),
			format!(
				":LUA:{kernel_name}#{}#{}",
				token.source().name(),
				matches.get(0).unwrap().start()
			),
			'\\',
			">@",
		);
		if source.content().is_empty() {
			report_warn!(
				&mut reports,
				token.source(),
				"Invalid Kernel Code".into(),
				span(script_range.clone(), "Kernel code is empty".into())
			);
		}

		let execute = |lua: &Lua| {
			let chunk = lua.load(source.content()).set_name(kernel_name);

			if index == 0
			// Exec @<>@
			{
				// Code Ranges
				if let Some(coderanges) = CodeRange::from_source(token.source(), &state.shared.lsp) {
					let range = script_range;
					coderanges.add(range.start + 1..range.end, "lua".to_string());
				}

				if let Err(e) = chunk.exec() {
					report_err!(
						&mut reports,
						source.clone(),
						"Invalid Kernel Code".into(),
						span(
							0..source.content().len(),
							format!("Kernel execution failed:\n{}", e)
						)
					);
				}
			} else
			// Eval %<>%
			{
				// Validate kind
				let kind = match matches.get(2) {
					None => 0,
					Some(kind) => match self.validate_kind(state.parser.colors(), kind.as_str()) {
						Ok(kind) => kind,
						Err(msg) => {
							report_err!(
								&mut reports,
								token.source(),
								"Invalid Kernel Code Kind".into(),
								span(kind.range(), msg)
							);
							return;
						}
					},
				};

				if kind == 0
				// Eval
				{
					if let Err(e) = chunk.eval::<()>() {
						report_err!(
							&mut reports,
							source.clone(),
							"Invalid Kernel Code".into(),
							span(
								0..source.content().len(),
								format!("Kernel evaluation failed:\n{}", e)
							)
						);
					}
				} else
				// Eval to string
				{
					match chunk.eval::<String>() {
						Ok(result) => {
							if kind == 1
							// Eval to text
							{
								if !result.is_empty() {
									state.push(
										document,
										Box::new(Text::new(
											Token::new(1..source.content().len(), source.clone()),
											process_text(document, result.as_str()),
										)),
									);
								}

								if let Some(hints) =
									Hints::from_source(token.source(), &state.shared.lsp)
								{
									hints.add(matches.get(0).unwrap().end(), result);
								}
							} else if kind == 2
							// Eval and Parse
							{
								if let Some(hints) =
									Hints::from_source(token.source(), &state.shared.lsp)
								{
									hints.add(matches.get(0).unwrap().end(), result.clone());
								}

								let parse_source = Arc::new(VirtualSource::new(
									Token::new(0..source.content().len(), source.clone()),
									format!(":LUA:parse({})", source.name()),
									result,
								)) as Arc<dyn Source>;

								state.with_state(|new_state| {
									new_state.parser.parse_into(
										new_state,
										parse_source,
										document,
										ParseMode::default(),
									);
								})
							}
						}
						Err(e) => {
							report_err!(
								&mut reports,
								source.clone(),
								"Invalid Kernel Code".into(),
								span(
									0..source.content().len(),
									format!("Kernel evaluation failed:\n{}", e)
								)
							);
						}
					}
				}
			}
		};

		let mut ctx = KernelContext::new(
			Token::new(0..source.content().len(), source.clone()),
			state,
			document,
		);

		kernel.run_with_context(&mut ctx, execute);

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			let range = matches
				.get(0)
				.map(|m| {
					if index == 0 && token.source().content().as_bytes()[m.start()] == b'\n' {
						m.start() + 1..m.end()
					} else {
						m.range()
					}
				})
				.unwrap();
			sems.add(range.start..range.start + 2, tokens.script_sep);
			if index == 0 {
				if let Some(kernel) = matches.get(1).map(|m| m.range()) {
					sems.add(kernel, tokens.script_kernel);
				}
			} else {
				if let Some(kernel) = matches.get(1).map(|m| m.range()) {
					sems.add(kernel.start - 1..kernel.start, tokens.script_kernel_sep);
					sems.add(kernel.clone(), tokens.script_kernel);
					sems.add(kernel.end..kernel.end + 1, tokens.script_kernel_sep);
				}
				if let Some(kind) = matches.get(2).map(|m| m.range()) {
					sems.add(kind, tokens.script_kind);
				}
			}
			sems.add(range.end - 2..range.end, tokens.script_sep);
		}

		// Process redirects as hints
		if let Some(hints) = Hints::from_source(token.source(), &state.shared.lsp) {
			let mut label = String::new();
			ctx.redirects.iter().for_each(|redir| {
				label += format!("{}: {} ", redir.source, redir.content).as_str();
			});
			if !label.is_empty() {
				label.pop();
				hints.add(matches.get(0).unwrap().end(), label);
			}
		}

		reports.extend(ctx.reports);
		reports
	}
}
