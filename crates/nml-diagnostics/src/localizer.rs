use std::fmt;

use fluent_bundle::FluentArgs;
use fluent_bundle::FluentResource;
use fluent_bundle::FluentValue;
use fluent_bundle::concurrent::FluentBundle;
use unic_langid::LanguageIdentifier;

use crate::Argument;
use crate::Diagnostic;
use crate::DiagnosticKind;
use crate::MessageContent;

/// The locale used when a message is missing from the requested one.
pub const FALLBACK_LOCALE: &str = "en-US";

/// The embedded `locales/<lang>/diagnostics.ftl` files.
pub(crate) const LOCALES: &[(&str, &str)] =
	&[("en-US", include_str!("../locales/en-US/diagnostics.ftl"))];

/// Formats diagnostics in a locale, falling back to [`FALLBACK_LOCALE`].
pub struct Localizer {
	/// The selected locale first, then the fallback.
	bundles: Vec<FluentBundle<FluentResource>>,
}

impl fmt::Debug for Localizer {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("Localizer")
			.field("locales", &self.locales())
			.finish()
	}
}

/// A diagnostic's text in one locale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalizedDiagnostic {
	pub title: String,
	/// One label per message of the diagnostic, in order.
	pub labels: Vec<Option<String>>,
	/// One title per fix, in order.
	pub fixes: Vec<String>,
}

impl Localizer {
	/// A localizer for `locale`, a language tag such as `fr-FR` or `fr`.
	///
	/// An exact match is preferred, then a locale with the same language. Unknown or invalid
	/// tags use the fallback only.
	pub fn new(locale: &str) -> Self {
		let requested = locale.parse::<LanguageIdentifier>().ok();
		let selected = requested.and_then(|requested| {
			let available = || {
				LOCALES
					.iter()
					.map(|(tag, _)| (*tag, tag.parse::<LanguageIdentifier>().unwrap()))
			};
			available()
				.find(|(_, id)| *id == requested)
				.or_else(|| available().find(|(_, id)| id.language == requested.language))
				.map(|(tag, _)| tag)
		});

		let mut tags: Vec<_> = selected.into_iter().collect();
		if selected != Some(FALLBACK_LOCALE) {
			tags.push(FALLBACK_LOCALE);
		}
		Self {
			bundles: tags.into_iter().map(bundle).collect(),
		}
	}

	/// A localizer for the locale of the environment: `LC_ALL`, `LC_MESSAGES`, then `LANG`.
	pub fn from_env() -> Self {
		let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
			.into_iter()
			.filter_map(|name| std::env::var(name).ok())
			.find(|value| !value.is_empty())
			.and_then(|value| posix_to_tag(&value))
			.unwrap_or_else(|| FALLBACK_LOCALE.to_string());
		Self::new(&locale)
	}

	/// The locales messages are looked up in, in order.
	pub fn locales(&self) -> Vec<String> {
		self.bundles
			.iter()
			.map(|bundle| bundle.locales[0].to_string())
			.collect()
	}

	/// The title of a diagnostic of this kind.
	pub fn title(&self, kind: &DiagnosticKind) -> String {
		self.format(kind, None, &[])
	}

	/// The text of a label or fix title.
	pub fn content(&self, kind: &DiagnosticKind, content: &MessageContent) -> String {
		match content {
			MessageContent::Fluent { attribute, args } => self.format(kind, Some(attribute), args),
			MessageContent::Text(text) => text.clone(),
		}
	}

	/// The title, labels and fix titles of a diagnostic.
	pub fn localize(&self, diagnostic: &Diagnostic) -> LocalizedDiagnostic {
		let kind = &diagnostic.kind;
		LocalizedDiagnostic {
			title: self.title(kind),
			labels: diagnostic
				.messages()
				.iter()
				.map(|message| {
					message
						.content
						.as_ref()
						.map(|content| self.content(kind, content))
				})
				.collect(),
			fixes: diagnostic
				.fixes
				.iter()
				.map(|fix| self.content(kind, &fix.title))
				.collect(),
		}
	}

	/// Formats the kind's message, or one of its attributes, in the first locale that has it.
	///
	/// A message missing from every locale is shown as its ID.
	fn format(
		&self,
		kind: &DiagnosticKind,
		attribute: Option<&str>,
		extra: &[(&'static str, Argument)],
	) -> String {
		let id = kind.message_id();
		let mut args = FluentArgs::new();
		for (name, value) in kind.args().into_iter().chain(extra.iter().cloned()) {
			args.set(name, fluent_value(value));
		}

		for bundle in &self.bundles {
			let Some(message) = bundle.get_message(id) else {
				continue;
			};
			let pattern = match attribute {
				Some(attribute) => message.get_attribute(attribute).map(|attr| attr.value()),
				None => message.value(),
			};
			if let Some(pattern) = pattern {
				let mut errors = Vec::new();
				return bundle
					.format_pattern(pattern, Some(&args), &mut errors)
					.into_owned();
			}
		}
		match attribute {
			Some(attribute) => format!("{id}.{attribute}"),
			None => id.to_string(),
		}
	}
}

fn bundle(tag: &str) -> FluentBundle<FluentResource> {
	let (_, source) = LOCALES.iter().find(|(name, _)| *name == tag).unwrap();
	let id: LanguageIdentifier = tag.parse().unwrap();
	let mut bundle = FluentBundle::new_concurrent(vec![id]);
	// Isolation marks would end up in terminal output.
	bundle.set_use_isolating(false);
	let resource = FluentResource::try_new(source.to_string())
		.unwrap_or_else(|_| panic!("locales/{tag}/diagnostics.ftl does not parse"));
	bundle
		.add_resource(resource)
		.unwrap_or_else(|_| panic!("locales/{tag}/diagnostics.ftl has duplicate messages"));
	bundle
}

fn fluent_value(value: Argument) -> FluentValue<'static> {
	match value {
		Argument::Text(text) => FluentValue::from(text),
		Argument::Number(number) => FluentValue::from(number),
	}
}

/// `fr_FR.UTF-8@euro` → `fr-FR`. `C` and `POSIX` have no language.
fn posix_to_tag(value: &str) -> Option<String> {
	let value = value.split(['.', '@']).next()?;
	match value {
		"" | "C" | "POSIX" => None,
		value => Some(value.replace('_', "-")),
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;

	use nml_source::Source;
	use nml_source::Token;
	use nml_source::Url;

	use crate::DiagnosticMessage;
	use crate::Fix;

	use super::*;

	fn token() -> Token {
		let source = Source::new(Url::parse("file:///a.nml").unwrap(), "a **b".into(), 0);
		Token::new(Arc::new(source), 2..4)
	}

	#[test]
	fn localize() {
		let diagnostic = Diagnostic::new(
			DiagnosticKind::ExpectedToken { token: "**".into() },
			DiagnosticMessage::fluent(token(), "opened").arg("style", "bold"),
		)
		.with(DiagnosticMessage::new(token()))
		.with(DiagnosticMessage::text(token(), "plain"))
		.with_fix(Fix {
			title: MessageContent::Fluent {
				attribute: "fix-escape",
				args: Vec::new(),
			},
			edits: Vec::new(),
		});

		let localized = Localizer::new("en-US").localize(&diagnostic);
		assert_eq!(localized.title, "expected `**`");
		assert_eq!(
			localized.labels,
			vec![Some("bold opened here".into()), None, Some("plain".into())]
		);
		assert_eq!(localized.fixes, vec!["Escape the opening delimiter"]);
	}

	#[test]
	fn missing_attribute_shows_id() {
		let kind = DiagnosticKind::UnexpectedToken { token: "x".into() };
		let content = MessageContent::Fluent {
			attribute: "nope",
			args: Vec::new(),
		};
		assert_eq!(
			Localizer::new("en-US").content(&kind, &content),
			"diagnostic-unexpected-token.nope"
		);
	}

	#[test]
	fn locale_selection() {
		assert_eq!(Localizer::new("en-US").locales(), ["en-US"]);
		assert_eq!(Localizer::new("en-GB").locales(), ["en-US"]);
		assert_eq!(Localizer::new("xx-YY").locales(), ["en-US"]);
		assert_eq!(Localizer::new("not a tag!").locales(), ["en-US"]);
	}

	#[test]
	fn posix_locales() {
		assert_eq!(posix_to_tag("fr_FR.UTF-8@euro").as_deref(), Some("fr-FR"));
		assert_eq!(posix_to_tag("de").as_deref(), Some("de"));
		assert_eq!(posix_to_tag("C.UTF-8"), None);
		assert_eq!(posix_to_tag("POSIX"), None);
	}
}
