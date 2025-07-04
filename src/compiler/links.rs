use std::collections::HashSet;

use crate::unit::unit::OffloadedUnit;
use crate::unit::unit::Reference;

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

/// Translate link from source unit to target unit
pub fn translate_reference(
	target: Target,
	from: &OffloadedUnit,
	to: &OffloadedUnit,
	reference: &Reference,
) -> String {
	match target {
		Target::HTML => {
			if from.output_path() == to.output_path() {
				format!("#{}", reference.link)
			} else {
				format!("{}#{}", from.output_path(), reference.link)
			}
		}
		_ => todo!(),
	}
}
