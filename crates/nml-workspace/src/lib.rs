//! Workspace configuration (`nml.toml`), file scanning, module resolution and the SQLite index.

mod config;

pub use config::CONFIG_FILE;
pub use config::Config;
pub use config::HtmlConfig;
pub use config::Module;
