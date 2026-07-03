//! Finding what is at a position of a document, through virtual sources.

use nml_grammar::Capture;
use nml_grammar::Node;
use nml_grammar::RuleMatch;
use nml_grammar::Value;
use nml_source::Token;
use nml_source::Url;

/// A match at a position, and its innermost capture there.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Hit<'t> {
	pub matched: &'t RuleMatch,
	pub capture: Option<&'t Capture>,
}

/// Whether a token, mapped to its file, is in `file` and contains `offset` (its end
/// included, so that a cursor right after a word is on it).
fn contains(token: &Token, file: &Url, offset: usize) -> bool {
	let root = token.root();
	root.source().location() == file && root.start() <= offset && offset <= root.end()
}

/// The matches that contain `offset` in the file, outermost first. Matches in virtual
/// sources (containers, Lua chunks) are found through their position in the file.
pub(crate) fn at<'t>(tree: &'t [Node], file: &Url, offset: usize) -> Vec<Hit<'t>> {
	let mut hits = Vec::new();
	walk(tree, file, offset, &mut hits);
	hits
}

fn walk<'t>(nodes: &'t [Node], file: &Url, offset: usize, hits: &mut Vec<Hit<'t>>) {
	for node in nodes {
		match node {
			Node::Paragraph { children, .. } => walk(children, file, offset, hits),
			Node::Match(m) if contains(&m.token, file, offset) => {
				let capture = m
					.captures
					.iter()
					.filter(|capture| contains(&capture.token, file, offset))
					// A capture that the offset is in wins over one that ends there; then the
					// innermost one.
					.min_by_key(|capture| {
						let root = capture.token.root();
						(root.end() == offset, root.range().len())
					});
				hits.push(Hit {
					matched: m,
					capture,
				});
				for capture in &m.captures {
					if let Value::Nodes(nodes) = &capture.value {
						walk(nodes, file, offset, hits);
					}
				}
			}
			_ => {}
		}
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use nml_grammar::Atom;
	use nml_grammar::Prefix;
	use nml_grammar::Rule;
	use nml_grammar::RuleSet;
	use nml_source::Source;

	use super::*;

	#[test]
	fn hits_through_containers() {
		let mut rules = RuleSet::new();
		let quote = rules
			.add(Rule::new(
				"quote",
				vec![
					Atom::LineStart,
					Atom::literal("> "),
					Atom::capture("body", Atom::Container(Prefix::Literal("> ".into()))),
				],
			))
			.unwrap();
		let bold = rules
			.add(Rule::new(
				"bold",
				vec![
					Atom::capture("open", Atom::literal("**")),
					Atom::capture("body", Atom::InlineContent),
					Atom::literal("**"),
				],
			))
			.unwrap();
		let url = Url::parse("file:///a.nml").unwrap();
		let source = Arc::new(Source::new(url.clone(), "x\n\n> a\n> b **cd**\n".into(), 0));
		let parsed = nml_grammar::parse(&rules, source);

		// On `c`, inside the bold, inside the quote's virtual source.
		let hits = at(&parsed.nodes, &url, 13);
		let rules_hit: Vec<_> = hits.iter().map(|hit| hit.matched.rule).collect();
		assert_eq!(rules_hit, [quote, bold]);
		let capture = hits[1].capture.unwrap();
		assert_eq!(&*capture.name, "body");
		assert_eq!(capture.token.root().content(), "cd");

		// On the opener.
		let hits = at(&parsed.nodes, &url, 11);
		assert_eq!(&*hits[1].capture.unwrap().name, "open");

		// Outside any match, or in another file.
		assert!(at(&parsed.nodes, &url, 0).is_empty());
		let other = Url::parse("file:///b.nml").unwrap();
		assert!(at(&parsed.nodes, &other, 13).is_empty());
	}
}
