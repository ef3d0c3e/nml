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
	/// it does not match the start of `:luax`.
	pub(crate) fn longest(&self, text: &str) -> Option<(RuleId, usize)> {
		let mut node = 0;
		let mut best = None;
		let mut chars = text.char_indices().peekable();
		while let Some((offset, c)) = chars.next() {
			let Some((_, child)) = self.nodes[node].children.iter().find(|(k, _)| *k == c) else {
				break;
			};
			node = *child;
			if let Some(rule) = self.nodes[node].rule {
				let at_boundary =
					!is_word_char(c) || chars.peek().is_none_or(|(_, next)| !is_word_char(*next));
				if at_boundary {
					best = Some((rule, offset + c.len_utf8()));
				}
			}
		}
		best
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

		assert_eq!(trie.longest("*a"), Some((RuleId(0), 1)));
		assert_eq!(trie.longest("**a"), Some((RuleId(1), 2)));
		assert_eq!(trie.longest("***"), Some((RuleId(1), 2)));
		assert_eq!(trie.longest("[[a"), Some((RuleId(3), 2)));
		// A longer path that does not complete falls back to the last opener on it.
		assert_eq!(trie.longest("[!y"), Some((RuleId(2), 1)));
		assert_eq!(trie.longest("a*"), None);
		assert_eq!(trie.longest(""), None);
	}

	#[test]
	fn word_openers_end_at_word_boundaries() {
		let mut trie = Trie::default();
		trie.insert(":lua", RuleId(0));
		trie.insert(":luax", RuleId(1));
		trie.insert(":", RuleId(2));

		assert_eq!(trie.longest(":lua`x`"), Some((RuleId(0), 4)));
		assert_eq!(trie.longest(":lua"), Some((RuleId(0), 4)));
		assert_eq!(trie.longest(":luax{"), Some((RuleId(1), 5)));
		assert_eq!(trie.longest(":luay"), Some((RuleId(2), 1)));
		assert_eq!(trie.longest(":lua_"), Some((RuleId(2), 1)));
		assert_eq!(trie.longest(":luaé"), Some((RuleId(2), 1)));
	}
}
