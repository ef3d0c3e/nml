use std::collections::HashMap;
use std::rc::Rc;

use downcast_rs::impl_downcast;
use downcast_rs::Downcast;

/// Styling for an element
pub trait ElementStyle: Downcast + core::fmt::Debug {
	/// The style key
	fn key(&self) -> &'static str;

	/// Attempts to create a new style from a [`json`] string
	///
	/// # Errors
	///
	/// Will fail if deserialization fails
	fn from_json(&self, json: &str) -> Result<Rc<dyn ElementStyle>, String>;

	/// Attempts to deserialize lua table into a new style
	fn from_lua(
		&self,
		lua: &mlua::Lua,
		value: mlua::Value,
	) -> Result<Rc<dyn ElementStyle>, mlua::Error>;
}
impl_downcast!(ElementStyle);

#[derive(Default)]
pub struct StyleHolder {
	styles: HashMap<String, Rc<dyn ElementStyle>>,
}

impl StyleHolder {
	/// Checks if a given style key is registered
	fn is_registered(&self, style_key: &str) -> bool { self.styles.contains_key(style_key) }

	/// Gets the current active style for an element
	/// NOTE: Will panic if a style is not defined for a given element
	/// If you need to process user input, use [`is_registered`]
	fn current(&self, style_key: &str) -> Rc<dyn ElementStyle> {
		self.styles.get(style_key).map(|rc| rc.clone()).unwrap()
	}

	/// Sets the [`style`]
	fn set_current(&mut self, style: Rc<dyn ElementStyle>) {
		self.styles.insert(style.key().to_string(), style);
	}
}

#[macro_export]
macro_rules! impl_elementstyle {
	($t:ty, $key:expr) => {
		impl crate::parser::style::ElementStyle for $t {
			fn key(&self) -> &'static str { $key }

			fn from_json(
				&self,
				json: &str,
			) -> Result<std::rc::Rc<dyn crate::parser::style::ElementStyle>, String> {
				serde_json::from_str::<$t>(json)
					.map_err(|e| e.to_string())
					.map(|obj| {
						std::rc::Rc::new(obj) as std::rc::Rc<dyn crate::parser::style::ElementStyle>
					})
			}

			fn from_lua(
				&self,
				lua: &mlua::Lua,
				value: mlua::Value,
			) -> Result<std::rc::Rc<dyn crate::parser::style::ElementStyle>, mlua::Error> {
				mlua::LuaSerdeExt::from_value::<$t>(lua, value).map(|obj| {
					std::rc::Rc::new(obj) as std::rc::Rc<dyn crate::parser::style::ElementStyle>
				})
			}
		}
	};
}
