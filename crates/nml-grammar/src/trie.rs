use crate::RuleId;

/// The rule openers of one table, for longest-match dispatch.
#[derive(Debug, Clone)]
pub(crate) struct Trie {
	nodes: Vec<Node>,
}

#[derive(Debug, Clone, Default)]
struct Node {
	children: Vec<(char, usize)>,
	rule: Option<RuleId>,
}

impl Default for Trie {
	fn default() -> Self {
		Self {
			nodes: vec![Node::default()],
		}
	}
}

impl Trie {
	pub(crate) fn insert(&mut self, opener: &str, rule: RuleId) {
		let mut node = 0;
		for c in opener.chars() {
			node = match self.nodes[node].children.iter().find(|(k, _)| *k == c) {
				Some((_, child)) => *child,
				None => {
					self.nodes.push(Node::default());
					let child = self.nodes.len() - 1;
					self.nodes[node].children.push((c, child));
					child
				}
			};
		}
		self.nodes[node].rule = Some(rule);
	}

	/// The rule with the longest opener at the start of `text`, and the opener's length in
	/// bytes.
	///
	/// An opener that ends with a word character (`:lua`) only matches where the word ends, so
	/// it does not match the start of `:luax`. For commands (`command` is set), the word must
	/// also be followed by a blank, a line break, the end, or `[`: at the start of a line,
	/// `` :lua`x` `` is a role in a paragraph, not the `:lua` command.
	pub(crate) fn longest(&self, text: &str, command: bool) -> Option<(RuleId, usize)> {
		let mut node = 0;
		let mut best = None;
		let mut chars = text.char_indices().peekable();
		while let Some((offset, c)) = chars.next() {
			let Some((_, child)) = self.nodes[node].children.iter().find(|(k, _)| *k == c) else {
				break;
			};
			node = *child;
			if let Some(rule) = self.nodes[node].rule {
				let at_boundary = !is_word_char(c)
					|| chars.peek().is_none_or(|(_, next)| {
						if command {
							next.is_whitespace() || *next == '['
						} else {
							!is_word_char(*next)
						}
					});
				if at_boundary {
					best = Some((rule, offset + c.len_utf8()));
				}
			}
		}
		best
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

	#[test]
	fn longest_match() {
		let mut trie = Trie::default();
		trie.insert("*", RuleId(0));
		trie.insert("**", RuleId(1));
		trie.insert("[", RuleId(2));
		trie.insert("[[", RuleId(3));
		trie.insert("[!x", RuleId(4));

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
		trie.insert(":lua", RuleId(0));
		trie.insert(":luax", RuleId(1));
		trie.insert(":", RuleId(2));

		assert_eq!(trie.longest_inline(":lua`x`"), Some((RuleId(0), 4)));
		assert_eq!(trie.longest_inline(":lua"), Some((RuleId(0), 4)));
		assert_eq!(trie.longest_inline(":luax{"), Some((RuleId(1), 5)));
		assert_eq!(trie.longest_inline(":luay"), Some((RuleId(2), 1)));
		assert_eq!(trie.longest_inline(":lua_"), Some((RuleId(2), 1)));
		assert_eq!(trie.longest_inline(":luaé"), Some((RuleId(2), 1)));
	}

	#[test]
	fn commands_end_with_a_blank_or_options() {
		let mut trie = Trie::default();
		trie.insert(":lua", RuleId(0));
		trie.insert("#", RuleId(1));
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
