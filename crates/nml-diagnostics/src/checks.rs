//! Checks the embedded locales against [`DiagnosticKind::KINDS`].

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::Path;

use fluent_syntax::ast;

use crate::DiagnosticKind;
use crate::localizer::FALLBACK_LOCALE;
use crate::localizer::LOCALES;

/// The prefix of every diagnostic message ID.
const PREFIX: &str = "diagnostic-";

struct Message<'a> {
	value: Option<&'a ast::Pattern<&'a str>>,
	attributes: HashMap<&'a str, &'a ast::Pattern<&'a str>>,
}

fn variables<'a>(pattern: &ast::Pattern<&'a str>, out: &mut BTreeSet<&'a str>) {
	for element in &pattern.elements {
		if let ast::PatternElement::Placeable { expression } = element {
			expression_variables(expression, out);
		}
	}
}

fn expression_variables<'a>(expression: &ast::Expression<&'a str>, out: &mut BTreeSet<&'a str>) {
	match expression {
		ast::Expression::Inline(inline) => inline_variables(inline, out),
		ast::Expression::Select { selector, variants } => {
			inline_variables(selector, out);
			for variant in variants {
				variables(&variant.value, out);
			}
		}
	}
}

fn inline_variables<'a>(inline: &ast::InlineExpression<&'a str>, out: &mut BTreeSet<&'a str>) {
	match inline {
		ast::InlineExpression::VariableReference { id } => {
			out.insert(id.name);
		}
		ast::InlineExpression::FunctionReference { arguments, .. } => {
			for argument in &arguments.positional {
				inline_variables(argument, out);
			}
			for argument in &arguments.named {
				inline_variables(&argument.value, out);
			}
		}
		ast::InlineExpression::Placeable { expression } => expression_variables(expression, out),
		_ => {}
	}
}

fn messages<'a>(
	tag: &str,
	resource: &'a ast::Resource<&'a str>,
	errors: &mut Vec<String>,
) -> HashMap<&'a str, Message<'a>> {
	let mut messages = HashMap::new();
	for entry in &resource.body {
		if let ast::Entry::Message(message) = entry {
			let id = message.id.name;
			let attributes = message
				.attributes
				.iter()
				.map(|attribute| (attribute.id.name, &attribute.value))
				.collect();
			let message = Message {
				value: message.value.as_ref(),
				attributes,
			};
			if messages.insert(id, message).is_some() {
				errors.push(format!("{tag}: `{id}` is defined twice"));
			}
		}
	}
	messages
}

fn check() -> Vec<String> {
	let mut errors = Vec::new();

	let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
	for entry in std::fs::read_dir(&directory).unwrap() {
		let name = entry.unwrap().file_name().into_string().unwrap();
		if !LOCALES.iter().any(|(tag, _)| *tag == name) {
			errors.push(format!("locales/{name} is not embedded in `LOCALES`"));
		}
	}

	let resources: Vec<_> = LOCALES
		.iter()
		.map(
			|(tag, source)| match fluent_syntax::parser::parse(*source) {
				Ok(resource) => (*tag, resource),
				Err((resource, parse_errors)) => {
					for error in parse_errors {
						errors.push(format!("{tag}: parse error: {error:?}"));
					}
					(*tag, resource)
				}
			},
		)
		.collect();
	let locales: Vec<_> = resources
		.iter()
		.map(|(tag, resource)| (*tag, messages(tag, resource, &mut errors)))
		.collect();
	let fallback = &locales
		.iter()
		.find(|(tag, _)| *tag == FALLBACK_LOCALE)
		.unwrap()
		.1;

	for (tag, messages) in &locales {
		for id in messages.keys() {
			if id.starts_with(PREFIX) && !DiagnosticKind::KINDS.iter().any(|k| k.message_id == *id)
			{
				errors.push(format!("{tag}: `{id}` is not used by any kind"));
			}
		}

		for kind in DiagnosticKind::KINDS {
			let id = kind.message_id;
			let Some(message) = messages.get(id) else {
				if *tag == FALLBACK_LOCALE {
					errors.push(format!("{tag}: `{id}` ({}) is missing", kind.name));
				}
				continue;
			};
			let fields: BTreeSet<_> = kind.fields.iter().copied().collect();

			let Some(value) = message.value else {
				errors.push(format!("{tag}: `{id}` has no value"));
				continue;
			};
			let mut in_value = BTreeSet::new();
			variables(value, &mut in_value);
			for variable in in_value.difference(&fields) {
				errors.push(format!(
					"{tag}: `{id}` uses `${variable}`, which is not a field of `{}`",
					kind.name
				));
			}

			// Attributes may also use the arguments of their label, so only the fields'
			// coverage is checked.
			let mut used = in_value;
			for pattern in message.attributes.values() {
				variables(pattern, &mut used);
			}
			for field in fields.difference(&used) {
				errors.push(format!(
					"{tag}: `{id}` never shows field `{field}` of `{}`",
					kind.name
				));
			}

			if *tag != FALLBACK_LOCALE {
				let known = fallback.get(id).map(|message| &message.attributes);
				for attribute in message.attributes.keys() {
					if !known.is_some_and(|known| known.contains_key(attribute)) {
						errors.push(format!(
							"{tag}: `{id}.{attribute}` is not in {FALLBACK_LOCALE}"
						));
					}
				}
			}
		}
	}
	errors
}

#[test]
fn locales_match_kinds() {
	let errors = check();
	assert!(errors.is_empty(), "\n{}", errors.join("\n"));
}
