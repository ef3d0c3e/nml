use regex::{Captures, Regex};
use crate::{document::element::Text, lua::kernel::{Kernel, KernelHolder}, parser::{parser::{Parser, ReportColors}, rule::RegexRule, source::{Source, Token, VirtualSource}, util}};
use ariadne::{Fmt, Label, Report, ReportKind};
use crate::document::document::Document;
use std::{ops::Range, rc::Rc};

pub struct ScriptRule
{
	re: [Regex; 2],
	eval_kinds: [(&'static str, &'static str); 2]
}

impl ScriptRule {
	pub fn new() -> Self {
		Self {
            re: [
				Regex::new(r"(?:^|\n)@<(?:(.*)\n?)((?:\\.|[^\[\]\\])*?)(?:\n?)>@").unwrap(),
				Regex::new(r"%<([^\s[:alpha:]])?(?:\[(.*?)\])?((?:\\.|[^\[\]\\])*?)(?:\n?)>%").unwrap()
			],
			eval_kinds: [
				("", "Eval to text"),
				("!", "Eval and parse"),
			]
        }
	}

	fn validate_kernel_name(colors: &ReportColors, name: &str)
		-> Result<String, String> {
		let trimmed = name.trim_end().trim_start();
		if trimmed.is_empty() { return Ok("main".to_string()) }
		else if trimmed.find(|c: char| c.is_whitespace()).is_some() { 
			return Err(format!("Kernel name `{}` contains whitespaces",
					trimmed.fg(colors.highlight)))
		}

		Ok(trimmed.to_string())
	}

	fn validate_kind(&self, colors: &ReportColors, kind: &str)
		-> Result<usize, String> {
		match self.eval_kinds.iter().position(|(kind_symbol, _)| kind == *kind_symbol)
		{
			Some(id) => Ok(id),
			None => Err(format!("Unable to find eval kind `{}`. Available kinds:{}",
					kind.fg(colors.highlight),
					self.eval_kinds.iter().fold(String::new(), |out, (symbol, name)| {
						out + format!("\n - '{symbol}' => {name}").as_str()
					})))
		}
	}
}

impl RegexRule for ScriptRule
{
    fn name(&self) -> &'static str { "Script" }

    fn regexes(&self) -> &[regex::Regex] { &self.re }

    fn on_regex_match(&self, index: usize, parser: &dyn Parser, document: &Document, token: Token, matches: Captures)
		-> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let kernel_name = match matches.get(if index == 0 {1} else {2}) {
			None => "main".to_string(),
			Some(name) => {
				match ScriptRule::validate_kernel_name(parser.colors(), name.as_str())
				{
					Ok(name) => name,
					Err(e) => {
						reports.push(
							Report::build(ReportKind::Error, token.source(), name.start())
							.with_message("Invalid kernel name")
							.with_label(
								Label::new((token.source(), name.range()))
								.with_message(e)
								.with_color(parser.colors().error))
							.finish());
						return reports;
					}
				}
			}
		};
		let kernel_name = matches.get(if index == 0 {1} else {2})
			.and_then(|name| {
				let trimmed = name.as_str().trim_start().trim_end();
				(!trimmed.is_empty()).then_some(trimmed)
			})
			.unwrap_or("main");
		let kernel = parser.get_kernel(kernel_name).unwrap_or_else(|| {
			parser.insert_kernel(kernel_name.to_string(), Kernel::new())
		});

		let kernel_data = matches.get(if index == 0 {2} else {3})
			.and_then(|code| { 
				let trimmed = code.as_str().trim_start().trim_end();
				(!trimmed.is_empty()).then_some((trimmed, code.range()))
			}).or_else(|| {
				reports.push(
					Report::build(ReportKind::Warning, token.source(), token.start())
					.with_message("Invalid kernel code")
					.with_label(
						Label::new((token.source(), token.start()+1..token.end()))
						.with_message("Kernel code is empty")
						.with_color(parser.colors().warning))
					.finish());

				None
			});

		if kernel_data.is_none() { return reports; }

		let (kernel_content, kernel_range) = kernel_data.unwrap();
		let source = Rc::new(VirtualSource::new(
			Token::new(kernel_range, token.source()),
			format!("{}#{}:lua_kernel@{kernel_name}", token.source().name(), matches.get(0).unwrap().start()),
			util::process_escaped('\\', ">@", kernel_content)
		)) as Rc<dyn Source>;
		
		let chunk = kernel.lua.load(source.content())
			.set_name(kernel_name);
		if index == 0 // @< ... >@ -> Exec
		{
			match chunk.exec()
			{
				Ok(_) => {},
				Err(e) => {
					reports.push(
						Report::build(ReportKind::Error, source.clone(), 0)
						.with_message("Invalid kernel code")
						.with_label(
							Label::new((source.clone(), 0..source.content().len()))
							.with_message(format!("Kernel execution failed:\n{}", e.to_string()))
							.with_color(parser.colors().error))
						.finish());
						}
			}
		}
		else if index == 1 // %< ... >% -> Eval
		{
			let kind = match matches.get(1) {
				None => 0,
				Some(kind) => {
					match self.validate_kind(parser.colors(), kind.as_str())
					{
						Ok(kind) => kind,
						Err(msg) => {
							reports.push(
								Report::build(ReportKind::Error, token.source(), kind.start())
								.with_message("Invalid kernel code kind")
								.with_label(
									Label::new((token.source(), kind.range()))
									.with_message(msg)
									.with_color(parser.colors().error))
								.finish());
							return reports;
						}
					}
				}
			};

			match chunk.eval::<String>()
			{
				Ok(result) => {
					if kind == 0 // Eval to text
					{
						if !result.is_empty()
						{
							parser.push(document, Box::new(Text::new(
							Token::new(1..source.content().len(), source.clone()),
							util::process_text(document, result.as_str()),
							)));
						}
					}
					else if kind == 1 // Eval and Parse
					{
						let parse_source = Rc::new(VirtualSource::new(
							Token::new(0..source.content().len(), source.clone()),
							format!("parse({})", source.name()),
							result
						)) as Rc<dyn Source>;
						//println!("SRC={parse_source:#?}, {}", parse_source.content());

						parser.parse_into(parse_source, document);
					}
				},
				Err(e) => {
					reports.push(
						Report::build(ReportKind::Error, source.clone(), 0)
						.with_message("Invalid kernel code")
						.with_label(
							Label::new((source.clone(), 0..source.content().len()))
							.with_message(format!("Kernel evaluation failed:\n{}", e.to_string()))
							.with_color(parser.colors().error))
						.finish());
				}
			}
		}

        reports
    }
}
