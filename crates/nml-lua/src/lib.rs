//! The sandboxed Lua 5.4 runtime: bindings, script rules, events and slots.

// `#[api]` refers to this crate as `::nml_lua`, also from inside it.
extern crate self as nml_lua;

mod api;
mod error;
mod runtime;
mod script;

pub use api::API;
pub use api::ApiFunction;
pub use api::TYPES_DIR;
pub use api::luacats;
pub use api::write_types;
pub use nml_lua_macros::api;
pub use runtime::Limits;
pub use runtime::Runtime;
pub use script::add;
