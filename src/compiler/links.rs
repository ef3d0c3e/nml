use std::collections::HashSet;

use super::compiler::Target;

/// Gets a unique link for a string for a given target
pub fn get_unique_link(
	target: Target,
	used_links: &mut HashSet<String>,
	refname: &String,
) -> String {
	// Replace illegal characters
	let mut transformed = match target {
		Target::HTML => refname
			.chars()
			.map(|c| {
				if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
					c
				} else if c == ' ' {
					'-'
				} else {
					'_'
				}
			})
			.collect::<String>(),
		_ => todo!(),
	};

	// Ensure uniqueness
	while used_links.contains(&transformed) {
		match target {
			Target::HTML => transformed.push('_'),
			_ => todo!(),
		}
	}
	used_links.insert(transformed.clone());
	transformed
}
