//! The generic rule engine: rules, dispatch, delimiter stack, captures and option schemas.

mod atom;
mod parser;
mod rule;
mod tree;
mod trie;

pub use atom::Atom;
pub use parser::Parsed;
pub use parser::parse;
pub use rule::Rule;
pub use rule::RuleError;
pub use rule::RuleErrorKind;
pub use rule::RuleId;
pub use rule::RuleSet;
pub use rule::Table;
pub use tree::Capture;
pub use tree::Node;
pub use tree::RuleMatch;
pub use tree::Value;
