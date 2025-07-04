use std::collections::HashMap;
use std::rc::Rc;

use downcast_rs::impl_downcast;
use downcast_rs::Downcast;

/// Styling for an element
///
/// Some elements have support for styling.
pub trait ElementStyle: Downcast + core::fmt::Debug {
	/// The style key
	fn key(&self) -> &'static str;

	/// Attempts to create a new style from a `json` string
	///
	/// # Errors
	///
	/// Will fail if deserialization fails
	fn from_json(&self, json: &str) -> Result<Rc<dyn ElementStyle>, String>;

	/// Attempts to deserialize a `lua table` into a new style
	fn from_lua(
		&self,
		lua: &mlua::Lua,
		value: mlua::Value,
	) -> Result<Rc<dyn ElementStyle>, mlua::Error>;
}
impl_downcast!(ElementStyle);

/// A structure that holds registered [`ElementStyle`]
pub struct StyleHolder {
	styles: HashMap<String, Rc<dyn ElementStyle>>,
}

macro_rules! create_styles {
	( $($construct:expr),+ $(,)? ) => {{
		let mut map = HashMap::new();
		$(
			let val = Rc::new($construct) as Rc<dyn ElementStyle>;
			map.insert(val.key().to_string(), val);
		)+
		map
	}};
}

//#[auto_registry::generate_registry(registry = "elem_styles", target = make_styles, return_type = HashMap<String, Rc<dyn ElementStyle>>, maker = create_styles)]
impl Default for StyleHolder {
	fn default() -> Self {
		Self {
			styles: HashMap::default(), //make_styles(),
		}
	}
}

impl StyleHolder {
	/// Checks if a given style key is registered
	pub fn is_registered(&self, style_key: &str) -> bool {
		self.styles.contains_key(style_key)
	}

	/// Gets the current active style for an element
	/// If you need to process user input, use [`Self::is_registered`]
	///
	/// # Notes
	///
	/// Will panic if a style is not defined for a given element.
	/// Elements should have their styles (when they support it) registered when the parser starts.
	pub fn current(&self, style_key: &str) -> Rc<dyn ElementStyle> {
		self.styles.get(style_key).cloned().unwrap()
	}

	/// Sets the style
	pub fn set_current(&mut self, style: Rc<dyn ElementStyle>) {
		self.styles.insert(style.key().to_string(), style);
	}
}

#[macro_export]
macro_rules! impl_elementstyle {
	($t:ty, $key:expr) => {
		impl $t {
			pub fn key() -> &'static str {
				$key
			}
		}
		impl $crate::parser::style::ElementStyle for $t {
			fn key(&self) -> &'static str {
				$key
			}

			fn from_json(
				&self,
				json: &str,
			) -> Result<std::rc::Rc<dyn $crate::parser::style::ElementStyle>, String> {
				serde_json::from_str::<$t>(json)
					.map_err(|e| e.to_string())
					.map(|obj| {
						std::rc::Rc::new(obj)
							as std::rc::Rc<dyn $crate::parser::style::ElementStyle>
					})
			}

			fn from_lua(
				&self,
				lua: &mlua::Lua,
				value: mlua::Value,
			) -> Result<std::rc::Rc<dyn $crate::parser::style::ElementStyle>, mlua::Error> {
				mlua::LuaSerdeExt::from_value::<$t>(lua, value).map(|obj| {
					std::rc::Rc::new(obj) as std::rc::Rc<dyn $crate::parser::style::ElementStyle>
				})
			}
		}
	};
}
