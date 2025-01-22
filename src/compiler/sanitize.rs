use super::compiler::Target;

/// The sanitizer is an object whose goal is to sanitize text for a given target
#[derive(Clone, Copy)]
pub struct Sanitizer {
	target: Target,
}

impl Sanitizer {
	pub fn new(target: Target) -> Self { Self { target } }

	pub fn target(&self) -> Target { self.target }

	pub fn sanitize<S: AsRef<str>>(&self, str: S) -> String {
		match self.target {
			Target::HTML => str
				.as_ref()
				.replace("&", "&amp;")
				.replace("<", "&lt;")
				.replace(">", "&gt;")
				.replace("\"", "&quot;"),
			_ => todo!("Sanitize not implemented"),
		}
	}

	pub fn sanitize_format<S: AsRef<str>>(&self, str: S) -> String {
		match self.target {
			Target::HTML => {
				let mut out = String::new();

				let mut braces = 0;
				for c in str.as_ref().chars() {
					if c == '{' {
						out.push(c);
						braces += 1;
						continue;
					} else if c == '}' {
						out.push(c);
						if braces != 0 {
							braces -= 1;
						}
						continue;
					}
					// Inside format args
					if braces % 2 == 1 {
						out.push(c);
						continue;
					}

					match c {
						'&' => out += "&amp;",
						'<' => out += "&lt;",
						'>' => out += "&gt;",
						'"' => out += "&quot;",
						_ => out.push(c),
					}
				}

				out
			}
			_ => todo!("Sanitize not implemented"),
		}
	}
}
