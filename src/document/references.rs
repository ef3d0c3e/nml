pub fn validate_refname(name: &str) -> Result<&str, String> {
	let trimmed = name.trim_start().trim_end();
	if trimmed.is_empty() {
		return Err("Refname cannot be empty".to_string());
	}

	for c in trimmed.chars() {
		if c.is_ascii_punctuation() {
			return Err(format!(
				"Refname `{trimmed}` cannot contain punctuation codepoint: `{c}`"
			));
		}

		if c.is_whitespace() {
			return Err(format!(
				"Refname `{trimmed}` cannot contain whitespaces: `{c}`"
			));
		}

		if c.is_control() {
			return Err(format!(
				"Refname `{trimmed}` cannot contain control codepoint: `{c}`"
			));
		}
	}

	Ok(trimmed)
}
