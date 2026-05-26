//! The generic rule engine: rules, dispatch, delimiter stack, captures and option schemas.

mod atom;
mod rule;
mod trie;

pub use atom::Atom;
pub use rule::Rule;
pub use rule::RuleError;
pub use rule::RuleErrorKind;
pub use rule::RuleId;
pub use rule::RuleSet;
pub use rule::Table;
