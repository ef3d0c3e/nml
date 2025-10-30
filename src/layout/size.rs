use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;

use crate::compiler::compiler::Target;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SizeOutput
{
	CSS,
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum Size {
	Em(f64),
	Px(f64),
	Percent(f64),
}

impl Size {
	pub fn to_output(&self, output: SizeOutput) -> String {
		match output {
			SizeOutput::CSS => match self {
				Size::Em(s) => format!("{s}em"),
				Size::Px(s) => format!("{s}px"),
				Size::Percent(s) => format!("{s}%"),
			},
			_ => todo!(),
		}
	}
}

impl ToString for Size {
	fn to_string(&self) -> String {
		match self {
			Size::Em(s) => format!("{s}em"),
			Size::Px(s) => format!("{s}px"),
			Size::Percent(s) => format!("{s}%"),
		}
	}
}

impl TryFrom<&str> for Size {
	type Error = String;

	fn try_from(s: &str) -> Result<Self, Self::Error> {
		let Some((sep, _)) = s.char_indices().find(|(_, c)| !c.is_numeric() || *c == '.') else {
			return Err(format!("Missing unit after size: '{}'", s));
		};
		let size = match s[0..sep].parse::<f64>() {
			Ok(size) => size,
			Err(err) => {
				return Err(format!(
					"Failed to parse '{}' as number: {}",
					&s[0..sep],
					err.to_string()
				))
			}
		};
		match s[sep..].trim_start().trim_end() {
			"em" => Ok(Size::Em(size)),
			"px" => Ok(Size::Px(size)),
			"%" => Ok(Size::Percent(size)),
			unit => return Err(format!("Unknown unit type: {}", unit)),
		}
	}
}
