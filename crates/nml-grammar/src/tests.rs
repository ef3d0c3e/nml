//! Tests of the parser as a whole: delimiter resolution, generated inputs and option lists.

use std::ops::Range;
use std::sync::Arc;

use nml_diagnostics::Diagnostic;
use nml_diagnostics::assert_diagnostics;

use crate::Atom;
use crate::Node;
use crate::OptionKind;
use crate::OptionSchema;
use crate::OptionSpec;
use crate::Options;
use crate::Rule;
use crate::RuleSet;
use crate::Value;
use crate::parse;
use crate::parser::tests::check;
use crate::parser::tests::check_graph;
use crate::parser::tests::graph_rules;
use crate::parser::tests::rules;
use crate::parser::tests::show;
use crate::parser::tests::source;

/// A small deterministic generator (xorshift64), so failures can be replayed.
struct Rng(u64);

impl Rng {
	fn next(&mut self) -> u64 {
		self.0 ^= self.0 << 13;
		self.0 ^= self.0 >> 7;
		self.0 ^= self.0 << 17;
		self.0
	}

	fn below(&mut self, n: usize) -> usize {
		(self.next() % n as u64) as usize
	}
}

mod delimiters {
	use super::*;

	/// The cases of the closing-chain rule, with the delimiter stack they exercise.
	#[test]
	fn chains_close_several_frames() {
		for (text, expected) in [
			// [*, **]: `*` then `**` covers 3, longer than the `**` opener.
			("***x***", r#"P[bold(body=[italic(body=["x"])])]"#),
			// [*, **], closed at the end of the bold's body.
			("**a *b***", r#"P[bold(body=["a " italic(body=["b"])])]"#),
			// [**, *]: `**` then `*`.
			("*a **b***", r#"P[italic(body=["a " bold(body=["b"])])]"#),
			// [*, **, *]: a chain of three frames.
			(
				"*a **b *c****",
				r#"P[italic(body=["a " bold(body=["b " italic(body=["c"])])])]"#,
			),
			// [**, *, **]: the chain covers 5.
			(
				"**a *b **c*****",
				r#"P[bold(body=["a " italic(body=["b " bold(body=["c"])])])]"#,
			),
			// The chain is only as long as needed: `***` closes two frames of three, and
			// the last `*` closes the outer italic.
			(
				"*a **b *c*** d*",
				r#"P[italic(body=["a " bold(body=["b " italic(body=["c"])]) " d"])]"#,
			),
		] {
			let diagnostics = check(text, expected);
			assert_diagnostics!(diagnostics, []);
		}
	}

	#[test]
	fn openers_win_over_shorter_chains() {
		for (text, expected) in [
			// [*]: the chain is `*` alone (1), shorter than `**` (2).
			(
				"*a **b** c*",
				r#"P[italic(body=["a " bold(body=["b"]) " c"])]"#,
			),
			// [*, **, *] at `*c`: only the outer italic would match, and `*` opens.
			(
				"*a **b *c* d** e*",
				r#"P[italic(body=["a " bold(body=["b " italic(body=["c"]) " d"]) " e"])]"#,
			),
			// [**, *, **] at `**c`.
			(
				"**a *b **c** d* e**",
				r#"P[bold(body=["a " italic(body=["b " bold(body=["c"]) " d"]) " e"])]"#,
			),
			// The same style cannot nest with text before the inner one: [*] at `*b`
			// closes, as the chain (1) is as long as the opener (1).
			(
				"*a *b* c*",
				r#"P[italic(body=["a "]) "b" italic(body=[" c"])]"#,
			),
		] {
			let diagnostics = check(text, expected);
			assert_diagnostics!(diagnostics, []);
		}
	}

	#[test]
	fn the_longest_opener_comes_first() {
		// A body that starts with another style is opened longest first, so italic inside
		// bold is the only reading of a leading `***`.
		let diagnostics = check("***a* b**", r#"P[bold(body=[italic(body=["a"]) " b"])]"#);
		assert_diagnostics!(diagnostics, []);
		// Bold inside italic needs an escape or a separator.
		let diagnostics = check(
			"*x **a** b*",
			r#"P[italic(body=["x " bold(body=["a"]) " b"])]"#,
		);
		assert_diagnostics!(diagnostics, []);
	}

	/// A style cannot directly contain the same style after some text: its delimiter closes
	/// first, as the chain is as long as the opener.
	#[test]
	fn same_style_closes_first() {
		let rules = rules();
		let parsed = parse(&rules, source("*a *b**"));
		assert_eq!(
			show(&rules, &parsed.nodes),
			r#"P[italic(body=["a "]) "b" bold!(body=[])]"#
		);
		assert_diagnostics!(parsed.diagnostics, [1001 @ 5..7]);

		let parsed = parse(&rules, source("**a **b** c**"));
		assert_eq!(
			show(&rules, &parsed.nodes),
			r#"P[bold(body=["a "]) "b" bold(body=[" c"])]"#
		);
		assert_diagnostics!(parsed.diagnostics, []);
	}

	#[test]
	fn chains_stop_at_verbatim_and_other_frames() {
		for (text, expected) in [
			// A code span is verbatim: its content does not close anything.
			(
				"**`a***`**",
				r#"P[bold(body=[code(fence="`" code="a***")])]"#,
			),
			// The link's `](` is not part of a chain of emphasis.
			(
				"[*a*](u) *[b](v)*",
				r#"P[url(text=[italic(body=["a"])] url="u") " " italic(body=[url(text=["b"] url="v")])]"#,
			),
			// [*, [ ]: `*` closes the italic, then `](` the link.
			(
				"[a *b*](u)",
				r#"P[url(text=["a " italic(body=["b"])] url="u")]"#,
			),
		] {
			let diagnostics = check(text, expected);
			assert_diagnostics!(diagnostics, []);
		}
	}

	#[test]
	fn outer_delimiters_end_unclosed_constructs() {
		// No opener at `](`: the bold is unclosed and ends there.
		let rules = rules();
		let parsed = parse(&rules, source("[a **b](u)"));
		assert_eq!(
			show(&rules, &parsed.nodes),
			r#"P[url(text=["a " bold!(body=["b"])] url="u")]"#
		);
		assert_diagnostics!(parsed.diagnostics, [1001 @ 3..5]);

		// An opener wins over an outer delimiter, even when it then stays unclosed.
		let parsed = parse(&rules, source("*a **b*"));
		assert_eq!(
			show(&rules, &parsed.nodes),
			r#"P[italic!(body=["a " bold!(body=["b" italic!(body=[])])])]"#
		);
		assert_diagnostics!(parsed.diagnostics, [1001 @ 6..7, 1001 @ 3..5, 1001 @ 0..1]);
	}
}

mod generated {
	use super::*;

	/// Checks that `nodes` cover `range` exactly, without gaps or overlaps, and returns the
	/// number of incomplete matches.
	fn check_cover(nodes: &[Node], range: Range<usize>, text: &str) -> usize {
		let mut pos = range.start;
		let mut incomplete = 0;
		for node in nodes {
			let token = node.token();
			assert_eq!(token.start(), pos, "gap or overlap in {text:?}");
			pos = token.end();
			match node {
				Node::Paragraph { children, token } => {
					incomplete += check_cover(children, token.range(), text);
				}
				Node::Match(m) => {
					incomplete += usize::from(!m.complete);
					for capture in &m.captures {
						let inside = capture.token.start() >= token.start()
							&& capture.token.end() <= token.end();
						assert!(inside, "capture outside its match in {text:?}");
						if let Value::Nodes(nodes) = &capture.value {
							incomplete += check_cover(nodes, capture.token.range(), text);
						}
					}
				}
				Node::Text(_) | Node::Escape(_) | Node::Comment(_) => {}
			}
		}
		assert_eq!(pos, range.end, "the nodes end early in {text:?}");
		incomplete
	}

	/// Random single-paragraph inputs over the inline syntax: the parser never panics, the
	/// tree covers the source, and there is one error per incomplete match.
	#[test]
	fn random_inline_input() {
		// A line break is followed by text, so no line is blank.
		const ALPHABET: &[&str] = &["*", "**", "x", " ", "`", "[", "](", ")", "\\", "\nx"];
		let rules = rules();
		let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
		for _ in 0..20_000 {
			let mut text = String::from("a");
			for _ in 0..rng.below(24) {
				text.push_str(ALPHABET[rng.below(ALPHABET.len())]);
			}
			let parsed = parse(&rules, source(&text));
			let [Node::Paragraph { children, token }] = parsed.nodes.as_slice() else {
				panic!("not one paragraph: {text:?}");
			};
			assert_eq!(token.range(), 0..text.len());
			let incomplete = check_cover(children, token.range(), &text);
			assert_eq!(
				incomplete,
				parsed.diagnostics.len(),
				"errors do not match incomplete matches in {text:?}"
			);
		}
	}

	#[derive(Debug)]
	enum Tree {
		Text(&'static str),
		Bold(Vec<Tree>),
		Italic(Vec<Tree>),
	}

	impl Tree {
		fn write(&self, out: &mut String) {
			let (marker, body) = match self {
				Tree::Text(text) => return out.push_str(text),
				Tree::Bold(body) => ("**", body),
				Tree::Italic(body) => ("*", body),
			};
			out.push_str(marker);
			body.iter().for_each(|tree| tree.write(out));
			out.push_str(marker);
		}

		fn show(trees: &[Tree]) -> String {
			let shown: Vec<_> = trees
				.iter()
				.map(|tree| match tree {
					Tree::Text(text) => format!("{text:?}"),
					Tree::Bold(body) => format!("bold(body=[{}])", Tree::show(body)),
					Tree::Italic(body) => format!("italic(body=[{}])", Tree::show(body)),
				})
				.collect();
			shown.join(" ")
		}
	}

	/// Text and styles, alternating, starting with text. A style is never directly inside
	/// the same style, and its body starts with text: those are the readings the syntax
	/// leaves to escapes. Bodies may end with a style, which needs a closing chain.
	fn body(rng: &mut Rng, depth: usize, parent: Option<bool>) -> Vec<Tree> {
		let mut trees = Vec::new();
		let texts = ["x", "x ", " x ", "ab"];
		for index in 0..1 + rng.below(4) {
			if index % 2 == 0 || depth == 0 {
				// Merge adjacent text, as the parser does.
				if !matches!(trees.last(), Some(Tree::Text(_))) {
					trees.push(Tree::Text(texts[rng.below(texts.len())]));
				}
				continue;
			}
			let bold = match parent {
				Some(parent) => !parent,
				None => rng.below(2) == 0,
			};
			let inner = body(rng, depth - 1, Some(bold));
			trees.push(if bold {
				Tree::Bold(inner)
			} else {
				Tree::Italic(inner)
			});
		}
		trees
	}

	/// Random well-nested emphasis parses back to the same tree, without errors.
	#[test]
	fn nested_emphasis_round_trips() {
		let rules = rules();
		let mut rng = Rng(0x2545_f491_4f6c_dd1d);
		for _ in 0..5_000 {
			let trees = body(&mut rng, 4, None);
			let mut text = String::new();
			trees.iter().for_each(|tree| tree.write(&mut text));
			let parsed = parse(&rules, source(&text));
			assert_eq!(
				show(&rules, &parsed.nodes),
				format!("P[{}]", Tree::show(&trees)),
				"in {text:?}"
			);
			assert!(parsed.diagnostics.is_empty(), "errors in {text:?}");
		}
	}
}

mod options {
	use super::*;

	fn parse_options(text: &str) -> (Options, Vec<Diagnostic>) {
		let rules = graph_rules();
		let parsed = parse(&rules, source(text));
		let Some(Node::Match(m)) = parsed.nodes.first() else {
			panic!("no match in {text:?}");
		};
		let Value::Options(options) = &m.capture("options").unwrap().value else {
			panic!("no options in {text:?}");
		};
		(options.clone(), parsed.diagnostics)
	}

	#[test]
	fn whitespace_and_line_breaks_are_skipped() {
		for text in [
			":graph[scale=2]",
			":graph[ scale = 2 ]",
			":graph[\n scale\n =\n 2\n]",
			":graph[\r\n\tscale =\r\n\t\t2\r\n]",
			":graph[scale=2,]",
			":graph[\n\tscale = 2,\n]",
			":graph[\n\n\tscale = 2\n\n]",
		] {
			let (options, diagnostics) = parse_options(text);
			assert_diagnostics!(diagnostics, []);
			assert_eq!(
				options.get("scale"),
				Some(&crate::OptionValue::Number(2.0)),
				"in {text:?}"
			);
		}

		let diagnostics = check_graph(
			":graph[\n\n]\n",
			r#"graph(options={layout:"dot" width:100 caption:"" inline:false})"#,
		);
		assert_diagnostics!(diagnostics, [1008]);
	}

	#[test]
	fn entries_keep_their_positions() {
		let text = ":graph[\n\tscale = 2,\n\tcaption =\n\t\t\"a, b\",\n]";
		let (options, diagnostics) = parse_options(text);
		assert_diagnostics!(diagnostics, []);
		let ranges: Vec<_> = options
			.entries
			.iter()
			.map(|entry| {
				(
					entry.key.content().to_string(),
					entry
						.value_token
						.as_ref()
						.map(|token| token.content().to_string()),
					entry.value.clone(),
				)
			})
			.collect();
		assert_eq!(
			ranges,
			[
				("scale".into(), Some("2".into()), Some("2".into())),
				(
					"caption".into(),
					Some("\"a, b\"".into()),
					Some("a, b".into())
				),
			]
		);
	}

	#[test]
	fn values_stay_on_one_line() {
		// A bare value ends at the end of its line, without trailing blanks.
		let (options, diagnostics) = parse_options(":graph[caption = a b  \n, scale=1]");
		assert_diagnostics!(diagnostics, []);
		assert_eq!(options.entries[0].value.as_deref(), Some("a b"));
		assert_eq!(
			options.entries[0].value_token.as_ref().unwrap().range(),
			17..20
		);

		// So the next line is not part of it.
		let rules = graph_rules();
		for (text, id, range) in [
			// The value ends with its line, and `x` is not `,` or `]`.
			(":graph[caption = a\nx]", 1001, 19..20),
			// A key is a single word: `sca` then `le`.
			(":graph[sca\nle=1]", 1001, 11..12),
			// A quoted value cannot cross a line.
			(":graph[caption = \"a\nb\"]", 1001, 19..20),
			// Separators without entries.
			(":graph[,]", 1003, 7..8),
			(":graph[scale=1,,]", 1003, 15..16),
			(":graph[\n,\n]", 1003, 8..9),
			// Unclosed at the end of the input.
			(":graph[\n\tscale = 1,\n", 1003, 20..20),
		] {
			let parsed = parse(&rules, source(text));
			nml_diagnostics::testing::check(&parsed.diagnostics, &[(id, Some(range))]);
		}
	}

	#[test]
	fn escapes_in_bare_values() {
		let (options, diagnostics) = parse_options(r":graph[caption = a\, b\]\\, scale=1]");
		assert_diagnostics!(diagnostics, []);
		assert_eq!(options.entries[0].value.as_deref(), Some(r"a, b]\"));
		let (options, _) = parse_options(r#":graph[caption = "a \"b\" \\", scale=1]"#);
		assert_eq!(options.entries[0].value.as_deref(), Some(r#"a "b" \"#));
	}

	/// An inline rule with options: the list can span the lines of its paragraph, not a
	/// blank line.
	#[test]
	fn inline_options_stay_in_their_paragraph() {
		let schema = OptionSchema::new([OptionSpec::new("x", OptionKind::Integer)]);
		let mut rules = RuleSet::new();
		rules
			.add(Rule::new(
				"role",
				vec![
					Atom::literal(":kbd"),
					Atom::capture("options", Atom::Options(Arc::new(schema))),
				],
			))
			.unwrap();

		let parsed = parse(&rules, source("a :kbd[\n  x = 1\n] b"));
		assert_eq!(
			show(&rules, &parsed.nodes),
			r#"P["a " role(options={x:1}) " b"]"#
		);
		assert_diagnostics!(parsed.diagnostics, []);

		let parsed = parse(&rules, source("a :kbd[\n\nx = 1]"));
		assert_eq!(
			show(&rules, &parsed.nodes),
			r#"P["a " role!()] P["x = 1]"]"#
		);
		assert_diagnostics!(parsed.diagnostics, [1003 @ 7..7]);
	}
}
