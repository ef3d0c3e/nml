use auto_userdata::AutoUserData;
use mlua::FromLua;
use mlua::LuaSerdeExt;
use serde::Deserialize;
use serde::Serialize;

use crate::parser::source::Token;

/// Name for references
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Refname {
	Internal(String),
	External(String, String),
	Bibliography(String, String),
}

impl FromLua for Refname {
    fn from_lua(value: mlua::Value, lua: &mlua::Lua) -> mlua::Result<Self> {
        lua.from_value(value)
    }
}


impl core::fmt::Display for Refname {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Refname::Internal(a) => write!(f, "{a}"),
			Refname::External(a, b) => write!(f, "{a}#{b}"),
			Refname::Bibliography(a, b) => write!(f, "{a}@{b}"),
		}
	}
}

impl TryFrom<&str> for Refname {
	type Error = String;

	fn try_from(s: &str) -> Result<Self, Self::Error> {
		if s.is_empty() {
			return Err("Refname cannot be empty".to_string());
		}

		// Validate
		let mut kind = None;
		let word = match s.split_once('#') {
			Some((_path, name)) => {
				kind = Some('#');
				name
			}
			None => match s.split_once('@') {
				Some((_path, name)) => {
					kind = Some('@');
					name
				}
				None => s,
			},
		};
		word.chars().try_for_each(|c| {
			if c == '#' || c == '@' {
				return Err(format!(
					"Refname `{s}` cannot contain `{c}` after previous specifier"
				));
			} else if c.is_ascii_punctuation() && !(c == '.' || c == '_') {
				return Err(format!(
					"Refname `{s}` cannot contain punctuation codepoint: `{c}`"
				));
			} else if c.is_whitespace() {
				return Err(format!("Refname `{s}` cannot contain whitespaces: `{c}`"));
			} else if c.is_control() {
				return Err(format!(
					"Refname `{s}` cannot contain control codepoint: `{c}`"
				));
			}

			Ok(())
		})?;
		match kind {
			Some('#') => {
				let p = s
					.split_once('#')
					.map(|(a, b)| (a.to_string(), b.to_string()))
					.unwrap();
				Ok(Self::External(p.0, p.1))
			}
			Some('@') => {
				let p = s
					.split_once('@')
					.map(|(a, b)| (a.to_string(), b.to_string()))
					.unwrap();
				Ok(Self::Bibliography(p.0, p.1))
			}
			_ => Ok(Self::Internal(s.to_string())),
		}
	}
}

/// References available inside a document
#[derive(Debug, AutoUserData)]
#[auto_userdata_target = "&"]
#[auto_userdata_target = "*"]
pub struct InternalReference {
	// Declaration
	location: Token,
	/// Name of the reference
	#[lua_value]
	refname: Refname,
}

impl InternalReference {
	pub fn new(location: Token, refname: Refname) -> Self {
		Self { location, refname }
	}

	pub fn name(&self) -> &Refname {
		&self.refname
	}

	pub fn location(&self) -> &Token {
		&self.location
	}
}
