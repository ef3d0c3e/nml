pub mod elem;
pub mod iterator;
pub mod luaudvec;
pub mod once_lock;
pub mod scope;
pub mod unit;
pub mod variable;
pub mod source;
pub mod internal_reference;

use std::sync::Arc;
use std::sync::OnceLock;

use parking_lot::RwLock;

use crate::parser::source::Source;
use crate::unit::element::Element;
use crate::unit::references::InternalReference;
use crate::unit::scope::Scope;
use crate::unit::translation::TranslationUnit;
use crate::unit::variable::Variable;

/// Wrapper for [`Variable`]
#[auto_registry::auto_registry(registry = "lua")]
pub struct VariableWrapper(pub Arc<dyn Variable>);

/// Wrapper for [`TranslationUnit`]
#[auto_registry::auto_registry(registry = "lua")]
pub struct UnitWrapper<'a>(pub &'a mut TranslationUnit);

/// Wrapper for [`Scope`]
#[auto_registry::auto_registry(registry = "lua")]
pub struct ScopeWrapper(pub Arc<RwLock<Scope>>);

/// Wrapper for [`Vec<Arc<RwLock<Scope>>>`]
#[auto_registry::auto_registry(registry = "lua")]
pub struct VecScopeWrapper(pub Vec<Arc<RwLock<Scope>>>);

/// Wrapper for [`Option<Arc<InternalReference>`]
#[auto_registry::auto_registry(registry = "lua")]
pub struct InternalReferenceWrapper(pub Option<Arc<InternalReference>>);

/// Wrapper for [`OnceLock`]
//#[auto_registry::auto_registry(registry = "lua")] TODO: Make it work for generic types
pub struct OnceLockWrapper<T>(pub OnceLock<T>);

/// Wrapper for [`Iterator`] over a [`Scope`]'s content
pub struct IteratorWrapper(pub Box<dyn Iterator<Item = (Arc<RwLock<Scope>>, Arc<dyn Element>)>>);

/// Wrapper for mutable [`Element`]
pub struct ElemMutWrapper<T>(pub T)
where
	T: Element,
	for <'a> &'a mut T: mlua::UserData;

/// Wrapper for [`Arc<dyn Element>`]
#[derive(Clone)]
pub struct ElemWrapper(pub Arc<dyn Element>);

/// Wrapper for a Vector of UserData objects
pub struct LuaUDVec<T>(pub Vec<T>);

/// Wrapper for [`Arc<dyn Source>`]
#[auto_registry::auto_registry(registry = "lua")]
pub struct SourceWrapper(pub Arc<dyn Source>);

pub trait UserDataElem
{
	fn take(lua: &mlua::Lua, ud: &mlua::AnyUserData) -> Result<Self, mlua::Error>
	where
		Self: Element + mlua::UserData + Send + Sync + 'static;
}
