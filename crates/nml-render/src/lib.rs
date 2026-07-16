//! External renderers (LaTeX, Graphviz, Typst) and code highlighting.

mod highlight;

pub use highlight::DEFAULT_THEME;
pub use highlight::highlight;
pub use highlight::is_known_language;
pub use highlight::languages;
pub use highlight::stylesheet;
pub use highlight::theme_names;
