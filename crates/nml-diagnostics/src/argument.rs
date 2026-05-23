use std::fmt;

/// A Fluent argument value.
#[derive(Debug, Clone, PartialEq)]
pub enum Argument {
	Text(String),
	Number(f64),
}

impl fmt::Display for Argument {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Argument::Text(text) => f.write_str(text),
			Argument::Number(number) => write!(f, "{number}"),
		}
	}
}

impl From<String> for Argument {
	fn from(value: String) -> Self {
		Argument::Text(value)
	}
}

impl From<&str> for Argument {
	fn from(value: &str) -> Self {
		Argument::Text(value.to_string())
	}
}

impl From<char> for Argument {
	fn from(value: char) -> Self {
		Argument::Text(value.to_string())
	}
}

macro_rules! from_number {
	($($ty:ty),*) => {
		$(impl From<$ty> for Argument {
			fn from(value: $ty) -> Self {
				Argument::Number(value as f64)
			}
		})*
	};
}

from_number!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize, f32, f64);
