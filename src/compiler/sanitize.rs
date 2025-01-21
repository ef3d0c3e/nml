use super::compiler::Target;

/// The sanitizer is an object whose goal is to sanitize text for a given target
#[derive(Clone, Copy)]
pub struct Sanitizer
{
	target: Target,
}

impl Sanitizer
{
	pub fn new(target: Target) -> Self {
		Self { target }
	}

	pub fn sanitize<S: AsRef<str>>(&self, s: S) -> String {
		match self.target {
			Target::HTML => s
				.as_ref()
				.replace("&", "&amp;")
				.replace("<", "&lt;")
				.replace(">", "&gt;")
				.replace("\"", "&quot;"),
			_ => todo!("Sanitize not implemented"),
		}
	}
}
