//! The HTML backend: compiles documents to templates with slots and fills them.

mod template;

pub use template::PageOptions;
pub use template::Template;
pub use template::TemplateSlot;
pub use template::compile;
pub use template::escape;
