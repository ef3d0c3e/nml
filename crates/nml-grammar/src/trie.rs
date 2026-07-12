use crate::RuleId;
use crate::rule::Opener;

/// The rule openers of one table, for longest-match dispatch.
#[derive(Debug, Clone)]
pub(crate) struct Trie {
	nodes: Vec<Node>,
}

#[derive(Debug, Clone, Default)]
struct Node {
	children: Vec<(Key, usize)>,
	rule: Option<RuleId>,
}

/// An edge of the trie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
	Char(char),
	/// One or more ASCII digits.
	Digits,
}

impl Default for Trie {
	fn default() -> Self {
		Self {
			nodes: vec![Node::default()],
		}
	}
}

impl Trie {
	pub(crate) fn insert(&mut self, opener: &Opener, rule: RuleId) {
		let keys = opener
			.digits
			.then_some(Key::Digits)
			.into_iter()
			.chain(opener.text.chars().map(Key::Char));
		let mut node = 0;
		for key in keys {
			node = match self.nodes[node].children.iter().find(|(k, _)| *k == key) {
				Some((_, child)) => *child,
				None => {
					self.nodes.push(Node::default());
					let child = self.nodes.len() - 1;
					self.nodes[node].children.push((key, child));
					child
				}
			};
		}
		self.nodes[node].rule = Some(rule);
	}

	/// The rule with the longest opener at the start of `text`, and the opener's length in
	/// bytes.
	///
	/// An opener that ends with a letter or digit (`:lua`) only matches where the word ends, so
	/// it does not match the start of `:luax`. For commands (`command` is set), the word must
	/// also be followed by a blank, a line break, the end, or `[`: at the start of a line,
	/// `` :lua`x` `` is a role in a paragraph, not the `:lua` command.
	pub(crate) fn longest(&self, text: &str, command: bool) -> Option<(RuleId, usize)> {
		let mut best = None;
		self.walk(0, text, 0, command, &mut best);
		best
	}

	/// Follows the edges of `node` that match `text` at `offset`, keeping the longest opener.
	fn walk(
		&self,
		node: usize,
		text: &str,
		offset: usize,
		command: bool,
		best: &mut Option<(RuleId, usize)>,
	) {
		let rest = &text[offset..];
		for (key, child) in &self.nodes[node].children {
			let (len, last) = match key {
				Key::Char(c) if rest.starts_with(*c) => (c.len_utf8(), *c),
				Key::Digits => {
					let len =
						rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
					if len == 0 {
						continue;
					}
					(len, '0')
				}
				_ => continue,
			};
			let end = offset + len;
			if let Some(rule) = self.nodes[*child].rule {
				let at_boundary = !last.is_alphanumeric()
					|| text[end..].chars().next().is_none_or(|next| {
						if command {
							next.is_whitespace() || next == '['
						} else {
							!is_word_char(next)
						}
					});
				if at_boundary && best.is_none_or(|(_, longest)| end > longest) {
					*best = Some((rule, end));
				}
			}
			self.walk(*child, text, end, command, best);
		}
	}
}

#[cfg(test)]
impl Trie {
	fn longest_inline(&self, text: &str) -> Option<(RuleId, usize)> {
		self.longest(text, false)
	}
}

pub(crate) fn is_word_char(c: char) -> bool {
	c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
	use super::*;

	fn text(text: &str) -> Opener {
		Opener {
			digits: false,
			text: text.into(),
		}
	}

	#[test]
	fn digit_openers() {
		let mut trie = Trie::default();
		let digits = Opener {
			digits: true,
			text: ". ".into(),
		};
		trie.insert(&digits, RuleId(0));
		trie.insert(&text("1"), RuleId(1));

		assert_eq!(trie.longest("1. a", true), Some((RuleId(0), 3)));
		assert_eq!(trie.longest("42. a", true), Some((RuleId(0), 4)));
		assert_eq!(trie.longest("1 x", true), Some((RuleId(1), 1)));
		assert_eq!(trie.longest("1.5", true), None);
		assert_eq!(trie.longest(". a", true), None);
		assert_eq!(trie.longest("2024 was", true), None);
	}

	#[test]
	fn longest_match() {
		let mut trie = Trie::default();
		trie.insert(&text("*"), RuleId(0));
		trie.insert(&text("**"), RuleId(1));
		trie.insert(&text("["), RuleId(2));
		trie.insert(&text("[["), RuleId(3));
		trie.insert(&text("[!x"), RuleId(4));

		assert_eq!(trie.longest_inline("*a"), Some((RuleId(0), 1)));
		assert_eq!(trie.longest_inline("**a"), Some((RuleId(1), 2)));
		assert_eq!(trie.longest_inline("***"), Some((RuleId(1), 2)));
		assert_eq!(trie.longest_inline("[[a"), Some((RuleId(3), 2)));
		// A longer path that does not complete falls back to the last opener on it.
		assert_eq!(trie.longest_inline("[!y"), Some((RuleId(2), 1)));
		assert_eq!(trie.longest_inline("a*"), None);
		assert_eq!(trie.longest_inline(""), None);
	}

	#[test]
	fn word_openers_end_at_word_boundaries() {
		let mut trie = Trie::default();
		trie.insert(&text(":lua"), RuleId(0));
		trie.insert(&text(":luax"), RuleId(1));
		trie.insert(&text(":"), RuleId(2));

		assert_eq!(trie.longest_inline(":lua`x`"), Some((RuleId(0), 4)));
		assert_eq!(trie.longest_inline(":lua"), Some((RuleId(0), 4)));
		assert_eq!(trie.longest_inline(":luax{"), Some((RuleId(1), 5)));
		assert_eq!(trie.longest_inline(":luay"), Some((RuleId(2), 1)));
		assert_eq!(trie.longest_inline(":lua_"), Some((RuleId(2), 1)));
		assert_eq!(trie.longest_inline(":luaé"), Some((RuleId(2), 1)));

		// `_` is a word character, but an opener ending with it is punctuation, like `*`.
		trie.insert(&text("__"), RuleId(3));
		assert_eq!(trie.longest_inline("__u"), Some((RuleId(3), 2)));
	}

	#[test]
	fn commands_end_with_a_blank_or_options() {
		let mut trie = Trie::default();
		trie.insert(&text(":lua"), RuleId(0));
		trie.insert(&text("#"), RuleId(1));
		for (text, expected) in [
			(":lua", Some((RuleId(0), 4))),
			(":lua\n", Some((RuleId(0), 4))),
			(":lua x", Some((RuleId(0), 4))),
			(":lua[a=1]", Some((RuleId(0), 4))),
			(":lua`x`", None),
			(":lua{x}", None),
			(":luax", None),
			// Openers that end with punctuation are not affected.
			("#`", Some((RuleId(1), 1))),
		] {
			assert_eq!(trie.longest(text, true), expected, "{text:?}");
		}
	}
}
