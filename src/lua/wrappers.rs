pub mod elem;
pub mod iterator;
pub mod luaudvec;
pub mod once_lock;
pub mod scope;
pub mod unit;
pub mod variable;
pub mod source;
pub mod internal_reference;
mod luaproxyvec;

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
pub struct VariableWrapper(pub *const Arc<dyn Variable>);

/// Wrapper for [`TranslationUnit`]
pub struct UnitWrapper(pub *mut TranslationUnit);

/// Wrapper for [`Scope`]
pub struct ScopeWrapper(pub *const Arc<RwLock<Scope>>);

/// Wrapper for [`Vec<Arc<RwLock<Scope>>>`]
pub struct VecScopeProxy(pub *const Vec<Arc<RwLock<Scope>>>);
pub struct VecScopeProxyMut(pub *mut Vec<Arc<RwLock<Scope>>>);

/// Wrapper for [`Option<Arc<InternalReference>`]
pub struct InternalReferenceOptProxy(pub *const Option<Arc<InternalReference>>);
pub struct InternalReferenceOptProxyMut(pub *mut Option<Arc<InternalReference>>);

/// Wrapper for [`OnceLock`]
pub struct OnceLockWrapper<T>(pub *const OnceLock<T>);

/// Wrapper for [`Iterator`] over a [`Scope`]'s content
pub struct IteratorWrapper(pub Box<dyn Iterator<Item = (Arc<RwLock<Scope>>, Arc<dyn Element>)>>);

/// Wrapper for [`Element`]
pub struct ElemWrapper(pub Arc<dyn Element>);

/// Wrapper for mutable [`Element`]
pub struct ElemWrapperMut(pub *mut dyn Element);

/// Wrapper for a Vector of UserData objects
pub struct LuaUdVecProxy<T>(pub *const Vec<T>);
pub struct LuaUdVecProxyMut<T>(pub *mut Vec<T>);

pub trait IntoLuaProxy
{
    type Proxy;
    type ProxyMut;

    fn as_proxy(ptr: *const Self) -> Self::Proxy;
    fn as_proxy_mut(ptr: *mut Self) -> Self::ProxyMut;
	fn from_proxy(proxy: &Self::Proxy) -> Self;
	fn from_proxy_mut(proxy: &Self::ProxyMut) -> Self;
}

/// Wrapper for a Vector of LuaProxy objects
pub struct LuaProxyVecProxy<T>(pub *const Vec<T>);
pub struct LuaProxyVecProxyMut<T>(pub *mut Vec<T>);
pub struct LuaProxyVecProxyOwned<T>(pub Vec<T>);

/// Wrapper for [`Arc<dyn Source>`]
pub struct SourceWrapper(pub *const Arc<dyn Source>);

pub trait UserDataElem
{
	fn take(lua: &mlua::Lua, ud: &mlua::AnyUserData) -> Result<Self, mlua::Error>
	where
		Self: Element + mlua::UserData + Send + Sync + 'static;
}
