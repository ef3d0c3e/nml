//! The HTML backend: compiles documents to templates with slots and fills them.

mod template;
mod writer;

pub use template::PageOptions;
pub use template::Template;
pub use template::TemplateSlot;
pub use template::compile;
pub use template::escape;
pub use writer::Resolved;
pub use writer::write;

#[cfg(test)]
mod tests;
