use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;

use ariadne::Fmt;
use regex::Regex;
use regex::RegexBuilder;
use url::Url;

use crate::elements::media::elem::Media;
use crate::elements::media::elem::MediaGroup;
use crate::elements::media::elem::MediaType;
use crate::layout::size::Size;
use crate::parser::property::Property;
use crate::parser::property::PropertyParser;
use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use crate::parser::rule::RegexRule;
use crate::parser::rule::RuleTarget;
use crate::parser::source::SourcePosition;
use crate::parser::source::Token;
use crate::parser::state::CustomStates;
use crate::parser::state::ParseMode;
use crate::parser::util::escape_source;
use crate::parser::util::parse_paragraph;
use crate::unit::references::InternalReference;
use crate::unit::references::Refname;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationAccessors;
use crate::unit::translation::TranslationUnit;

#[auto_registry::auto_registry(registry = "rules")]
pub struct MediaRule {
	re: [Regex; 1],
	properties: PropertyParser,
}

impl Default for MediaRule {
	fn default() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"type".to_string(),
			Property::new("Override for the media type detection".to_string(), None),
		);
		props.insert(
			"width".to_string(),
			Property::new("Override for the media width".to_string(), None),
		);
		props.insert(
			"caption".to_string(),
			Property::new("Medium caption".to_string(), None),
		);
		Self {
			re: [RegexBuilder::new(
				r"^!\[(.*)\]\(((?:\\.|[^\\\\])*?)\)(?:\[((?:\\.|[^\\\\])*?)\])?((?:\\(?:.|\n)|[^\\\\])*?$)?",
			)
			.multi_line(true)
			.build()
			.unwrap()],
			properties: PropertyParser { properties: props },
		}
	}
}

impl RegexRule for MediaRule {
	fn name(&self) -> &'static str {
		"Media"
	}

	fn target(&self) -> crate::parser::rule::RuleTarget {
		RuleTarget::Block
	}

	fn regexes(&self) -> &[regex::Regex] {
		&self.re
	}

	fn enabled(
		&self,
		_unit: &TranslationUnit,
		mode: &ParseMode,
		_states: &mut CustomStates,
		_index: usize,
	) -> bool {
		return !mode.paragraph_only;
	}

	fn on_regex_match<'u>(
		&self,
		_index: usize,
		unit: &mut TranslationUnit,
		token: crate::parser::source::Token,
		captures: regex::Captures,
	) {
		// Parse refname
		let refname_group = captures.get(1).unwrap();
		unit.with_lsp(|lsp| {
			lsp.with_semantics(token.source(), |sems, tokens| {
				sems.add(
					captures.get(0).unwrap().start()..captures.get(0).unwrap().start() + 1,
					tokens.media_sep,
				);
				sems.add(
					refname_group.range().start - 1..refname_group.range().start,
					tokens.media_refname_sep,
				);
				sems.add(
					refname_group.range().start..refname_group.range().end,
					tokens.media_refname,
				);
				sems.add(
					refname_group.range().end..refname_group.range().end + 1,
					tokens.media_refname_sep,
				);
			});
		});

		let refname = match Refname::try_from(refname_group.as_str()) {
			Ok(refname) => match refname {
				Refname::Internal(refname) => Refname::Internal(refname),
				Refname::External(_, _) | Refname::Bibliography(_, _) => {
					report_err!(
						unit,
						token.source(),
						"Invalid Media Refname".into(),
						span(refname_group.range(), format!("Expected internal refname"))
					);
					return;
				}
			},
			Err(err) => {
				report_err!(
					unit,
					token.source(),
					"Invalid Media Refname".into(),
					span(refname_group.range(), err.into())
				);
				return;
			}
		};

		// Parse url
		let url_group = captures.get(2).unwrap();
		unit.with_lsp(|lsp| {
			lsp.with_semantics(token.source(), |sems, tokens| {
				sems.add(
					url_group.range().start - 1..url_group.range().start,
					tokens.media_url_sep,
				);
				sems.add(
					url_group.range().start..url_group.range().end,
					tokens.media_url,
				);
				sems.add(
					url_group.range().end..url_group.range().end + 1,
					tokens.media_url_sep,
				);
			});
		});
		let url = match Url::from_str(url_group.as_str()) {
			Ok(url) => url,
			Err(err) => match err {
				url::ParseError::RelativeUrlWithoutBase => {
					let mut path = PathBuf::from(url_group.as_str());
					if !path.is_absolute() {
						let Some(cwd) = unit.output_path().cloned() else {
							report_err!(
								unit,
								token.source(),
								"Invalid Media Url".into(),
								span(
									url_group.range(),
									format!("Cannot specify a relative Url without knowing the unit's output path")
								)
							);
							return;
						};
						path = cwd;
						path.pop();
						path.push(url_group.as_str());
					}
					if !path.exists() {
						report_warn!(
							unit,
							token.source(),
							"Media Path does not Exist".into(),
							span(
								url_group.range(),
								format!(
									"Path `{}` does not exist",
									path.display().fg(unit.colors().info)
								)
							)
						);
					}
					match Url::from_file_path(&path) {
						Ok(url) => url,
						Err(()) => {
							report_err!(
								unit,
								token.source(),
								"Invalid Media Url".into(),
								span(
									url_group.range(),
									format!(
										"Path `{}` is not valid",
										path.display().fg(unit.colors().info)
									)
								)
							);
							return;
						}
					}
				}
				_ => {
					report_err!(
						unit,
						token.source(),
						"Invalid Media Url".into(),
						span(
							url_group.range(),
							format!("Url/Path is not valid for media: {err}")
						)
					);
					return;
				}
			},
		};

		// Parse properties
		if let Some(prop_group) = captures.get(3) {
			unit.with_lsp(|lsp| {
				lsp.with_semantics(token.source(), |sems, tokens| {
					sems.add(
						prop_group.range().start - 1..prop_group.range().start,
						tokens.media_prop_sep,
					);
					sems.add_to_queue(
						prop_group.range().end..prop_group.range().end + 1,
						tokens.media_prop_sep,
					);
				});
			});
		}
		let prop_source = escape_source(
			token.source(),
			captures.get(3).map_or(0..0, |m| m.range()),
			PathBuf::from("Media Properties"),
			'\\',
			"]",
		);
		let Some(mut properties) = self.properties.parse(
			"Raw Code",
			unit,
			Token::new(0..prop_source.content().len(), prop_source),
		) else {
			return;
		};

		let (Some(media_type), Some(caption), Some(width)) = (
			properties.get_or_else(
				unit,
				"type",
				|| MediaType::from_filename(url.as_str()).ok_or(String::default()),
				|_, value| Result::<_, String>::Ok(MediaType::try_from(value.value.as_str())),
			),
			properties.get_opt(unit, "caption", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}),
			properties.get_opt(unit, "width", |_, value| {
				Size::try_from(value.value.as_str())
			}),
		) else {
			return;
		};

		let media_type = match media_type {
			Ok(media_type) => media_type,
			Err(err) => {
				report_err!(
					unit,
					token.source(),
					"Invalid Media Type".into(),
					span(
						url_group.range(),
						format!(
							"Failed to detect media type for `{}`: {err}",
							url.as_str().fg(unit.colors().info)
						)
					)
				);
				return;
			}
		};

		let description_group = captures.get(4).unwrap();
		let description = {
			let desc_source = escape_source(
				token.source(),
				description_group.range(),
				PathBuf::from(format!("Media[{refname}] description")),
				'\\',
				"\n",
			);
			if desc_source.content().is_empty() {
				None
			} else {
				match parse_paragraph(unit, desc_source) {
					Err(err) => {
						report_err!(
							unit,
							token.source(),
							"Invalid Media Description".into(),
							span(
								description_group.range(),
								format!("Failed to parse media description:\n{err}")
							)
						);
						return;
					}
					Ok(paragraph) => Some(paragraph),
				}
			}
		};

		let reference = Some(Arc::new(InternalReference::new(
			token.source().original_range(token.range.clone()),
			refname,
		)));
		println!("size={width:#?}");
		let media = Media {
			location: token.clone(),
			url,
			media_type,
			width,
			caption,
			description,
			reference,
			link: OnceLock::default(),
		};
		let mut has_group = false;
		if let Some(elem) = unit.get_scope().content_last() {
			if let Some(_) = elem.downcast_ref::<MediaGroup>() {
				has_group = true
			}
		}

		let mut group = if has_group {
			let group = unit.get_scope().take_last_content().unwrap();
			let group = Arc::downcast::<MediaGroup>(group).unwrap();
			Arc::try_unwrap(group).expect("Failed to take back ownerwship of MediaGroup")
		} else {
			MediaGroup {
				location: token.clone(),
				media: vec![],
			}
		};
		group.media.push(Arc::new(media));
		unit.add_content(group);
	}
}
