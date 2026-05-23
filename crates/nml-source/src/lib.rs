//! Sources, tokens and position mapping, including virtual sources.

mod position;
mod source;
mod token;
mod virtual_source;

pub use position::Encoding;
pub use position::Position;
pub use source::Source;
pub use token::Token;
pub use url::Url;
pub use virtual_source::Bias;
pub use virtual_source::VirtualSourceBuilder;
