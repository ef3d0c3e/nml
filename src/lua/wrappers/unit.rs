use std::path::PathBuf;
use std::sync::Arc;

use crate::add_documented_method;
use crate::cache::cache::Cache;
use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::compiler::process::ProcessQueue;
use crate::lua::kernel::Kernel;
use crate::lua::wrappers::ElemWrapper;
use crate::lua::wrappers::IteratorWrapper;
use crate::lua::wrappers::ScopeWrapper;
use crate::lua::wrappers::UnitWrapper;
use crate::lua::wrappers::VariableWrapper;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;
use crate::unit::variable::VariableName;
use crate::util::settings::ProjectSettings;
use graphviz_rust::print;
use mlua::UserData;

impl UserData for UnitWrapper {
	fn add_methods<M: mlua::UserDataMethods<Self>>(methods: &mut M) {
		methods.add_method("entry_scope", |_lua, this, ()| {
			let r = unsafe { &mut *this.0 as &mut TranslationUnit };
			Ok(ScopeWrapper(r.get_entry_scope().clone()))
		});
		methods.add_method("content", |_lua, this, (recurse,): (bool,)| {
			let r = unsafe { &mut *this.0 as &mut TranslationUnit };
			let it = r.get_entry_scope().content_iter(recurse);

			Ok(IteratorWrapper(Box::new(it)))
		});
		methods.add_method("get_variable", |_lua, this, (name,): (String,)| {
			let r = unsafe { &mut *this.0 as &mut TranslationUnit };
			let Some((var, _)) = r.get_entry_scope().get_variable(&VariableName(name)) else {
				return Ok(None);
			};
			Ok(Some(VariableWrapper(var)))
		});
		add_documented_method!(
			methods,
			"Unit",
			"add_content",
			|lua, this, (elem,): (ElemWrapper,)| {
				let r = unsafe { &mut *this.0 as &mut TranslationUnit };
				Kernel::with_context(lua, |ctx| {
					r.add_content_raw(elem.0.clone());
					if let Some(reference) = elem.0.clone().as_referenceable() {
						r.add_reference(reference);
					}
					if let Some(container) = elem.0.as_container() {
						for scope in container.contained() {
							for (scope, elem) in scope.content_iter(true) {
								if let Some(reference) = elem.as_referenceable() {
									r.add_reference(reference);
								}
							}
						}
					}
				});
				Ok(())
			},
			"Insert content in the unit at the current position",
			vec!["self", "elem:Element Element to insert"],
			None
		);
		methods.add_method("compile", |lua, this, (format, file): (String, String)| {
			// TODO: assert is_meta
			let r = unsafe { &mut *this.0 as &mut TranslationUnit };
			let target =
				Target::try_from(format.as_str()).map_err(|err| mlua::Error::BadArgument {
					to: Some("compile".into()),
					pos: 1,
					name: Some("format".into()),
					cause: Arc::new(mlua::Error::runtime(err)),
				})?;
			let (content, mut output) = Kernel::with_context(lua, |ctx| {
				let cache = ctx.kernel.cache.clone().unwrap_or_else(|| {
					Arc::new(Cache::new(&PathBuf::default()).unwrap())
				});
				let compiler = Compiler::new(target, cache);
				let output = ctx.unit.get_settings().output_path.clone();
				let content = compiler.compile(r).map_err(|mut err| {
					for report in err.drain(..) {
						ctx.unit.report(report);
					}
					mlua::Error::runtime("Failed to compile unit".to_string())
				})?;
				Ok::<_, mlua::Error>((content, output))
			})?;
			
			output.push(file);
			std::fs::write(&output, content).map_err(|err| {
					mlua::Error::runtime(format!("Failed to output unit to {}", output.display()))
			})?;
			println!("Compiled unit {}", output.display());

			Ok(())
		});
	}
}
