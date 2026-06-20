//! Workspace configuration (`nml.toml`), file scanning, module resolution and the SQLite index.

mod config;
mod index;
mod scan;

pub use config::CONFIG_FILE;
pub use config::Config;
pub use config::HtmlConfig;
pub use config::Module;
pub use index::FileState;
pub use index::Index;
pub use index::IndexError;
pub use index::Role;
pub use index::SCHEMA_VERSION;
pub use index::StoredSlot;
pub use index::StoredTemplate;
pub use index::StoredUnit;
pub use index::StoredUse;
pub use index::UnitRecord;
pub use scan::Change;
pub use scan::Scan;
pub use scan::ScanError;
pub use scan::ScannedFile;
pub use scan::hash;
pub use scan::scan;
