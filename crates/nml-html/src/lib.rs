//! The HTML backend: compiles documents to templates with slots and fills them.

mod template;
mod writer;

pub use template::CODE_STYLESHEET;
pub use template::DEFAULT_STYLE;
pub use template::DEFAULT_STYLESHEET;
pub use template::PageOptions;
pub use template::Template;
pub use template::TemplateSlot;
pub use template::compile;
pub use template::escape;
pub use template::fragment;
pub use writer::Resolved;
pub use writer::write;

#[cfg(test)]
mod tests;
