use std::collections::HashMap;

use ariadne::Fmt;
use document::document::Document;
use document::document::DocumentAccessors;
use document::element::ContainerElement;
use document::references::validate_refname;
use lsp::semantic::Semantics;
use parser::parser::ParseMode;
use parser::parser::ParserState;
use parser::property::Property;
use parser::property::PropertyParser;
use parser::rule::RegexRule;
use parser::source::Token;
use parser::util::escape_source;
use parser::util::escape_text;
use parser::util::parse_paragraph;
use regex::Captures;
use regex::Regex;
use regex::RegexBuilder;

use crate::parser::reports::macros::*;
use crate::parser::reports::*;
use std::str::FromStr;

use super::elem::Media;
use super::elem::MediaType;
use super::elem::Medium;

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

fn validate_uri(uri: &str) -> Result<&str, String> {
	let trimmed = uri.trim_start().trim_end();

	if trimmed.is_empty() {
		return Err("URIs is empty".to_string());
	}

	Ok(trimmed)
}

fn detect_filetype(filename: &str) -> Option<MediaType> {
	let sep = match filename.rfind('.') {
		Some(pos) => pos,
		None => return None,
	};

	// TODO: https://developer.mozilla.org/en-US/docs/Web/Media/Formats/Containers
	match filename.split_at(sep + 1).1.to_ascii_lowercase().as_str() {
		"png" | "apng" | "avif" | "gif" | "webp" | "svg" | "bmp" | "jpg" | "jpeg" | "jfif"
		| "pjpeg" | "pjp" => Some(MediaType::IMAGE),
		"mp4" | "m4v" | "webm" | "mov" => Some(MediaType::VIDEO),
		"mp3" | "ogg" | "flac" | "wav" => Some(MediaType::AUDIO),
		_ => None,
	}
}

impl RegexRule for MediaRule {
	fn name(&self) -> &'static str {
		"Media"
	}

	fn previous(&self) -> Option<&'static str> {
		Some("Graphviz")
	}

	fn regexes(&self) -> &[regex::Regex] {
		&self.re
	}

	fn enabled(&self, mode: &ParseMode, _id: usize) -> bool {
		!mode.paragraph_only
	}

	fn on_regex_match<'a>(
		&self,
		_: usize,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
		token: Token,
		matches: Captures,
	) -> Vec<Report> {
		let mut reports = vec![];

		let refname = match (
			matches.get(1).unwrap(),
			validate_refname(document, matches.get(1).unwrap().as_str(), true),
		) {
			(_, Ok(refname)) => refname.to_string(),
			(m, Err(err)) => {
				report_err!(
					&mut reports,
					token.source(),
					"Invalid Media Refname".into(),
					span(m.range(), err)
				);
				return reports;
			}
		};

		let uri = match (
			matches.get(2).unwrap(),
			validate_uri(matches.get(2).unwrap().as_str()),
		) {
			(_, Ok(uri)) => escape_text('\\', ")", uri, true),
			(m, Err(err)) => {
				report_err!(
					&mut reports,
					token.source(),
					"Invalid Media URI".into(),
					span(m.range(), err)
				);
				return reports;
			}
		};

		// Properties
		let prop_source = escape_source(
			token.source(),
			matches.get(3).map_or(0..0, |m| m.range()),
			"Media Properties".into(),
			'\\',
			"]",
		);
		let properties = match self.properties.parse(
			"Media",
			&mut reports,
			state,
			Token::new(0..prop_source.content().len(), prop_source),
		) {
			Some(props) => props,
			None => return reports,
		};

		let (media_type, caption, width) = match (
			properties.get_opt(&mut reports, "type", |_, value| {
				MediaType::from_str(value.value.as_str())
			}),
			properties.get_opt(&mut reports, "caption", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}),
			properties.get_opt(&mut reports, "width", |_, value| {
				Result::<_, String>::Ok(value.value.clone())
			}),
		) {
			(Some(media_type), Some(caption), Some(width)) => {
				if media_type.is_none() {
					match detect_filetype(uri.as_str()) {
						None => {
							report_err!(
								&mut reports,
								token.source(),
								"Invalid Media Property".into(),
								span(
									token.start() + 1..token.end(),
									format!(
										"Failed to detect media type for `{}`",
										uri.fg(state.parser.colors().info)
									)
								)
							);
							return reports;
						}
						Some(media_type) => (media_type, caption, width),
					}
				} else {
					(media_type.unwrap(), caption, width)
				}
			}
			_ => return reports,
		};

		if let Some((sems, tokens)) = Semantics::from_source(token.source(), &state.shared.lsp) {
			sems.add(
				matches.get(0).unwrap().start()..matches.get(0).unwrap().start() + 1,
				tokens.media_sep,
			);
			// Refname
			sems.add(
				matches.get(0).unwrap().start() + 1..matches.get(0).unwrap().start() + 2,
				tokens.media_refname_sep,
			);
			sems.add(matches.get(1).unwrap().range(), tokens.media_refname);
			sems.add(
				matches.get(1).unwrap().end()..matches.get(1).unwrap().end() + 1,
				tokens.media_refname_sep,
			);
			// Uri
			sems.add(
				matches.get(2).unwrap().start() - 1..matches.get(2).unwrap().start(),
				tokens.media_uri_sep,
			);
			sems.add(matches.get(2).unwrap().range(), tokens.media_uri);
			sems.add(
				matches.get(2).unwrap().end()..matches.get(2).unwrap().end() + 1,
				tokens.media_uri_sep,
			);
			// Props
			if let Some(props) = matches.get(3) {
				sems.add(props.start() - 1..props.start(), tokens.media_props_sep);
				sems.add(props.end()..props.end() + 1, tokens.media_props_sep);
			}
		}

		let description = match matches.get(4) {
			Some(content) => {
				let source = escape_source(
					token.source(),
					content.range(),
					format!("Media[{refname}] description"),
					'\\',
					"\n",
				);
				if source.content().is_empty() {
					None
				} else {
					match parse_paragraph(state, source, document) {
						Ok(paragraph) => Some(*paragraph),
						Err(err) => {
							report_err!(
								&mut reports,
								token.source(),
								"Invalid Media Description".into(),
								span(
									content.range(),
									format!("Could not parse description: {err}")
								)
							);
							return reports;
						}
					}
				}
			}
			None => panic!("Unknown error"),
		};

		let mut group = match document.last_element_mut::<Media>() {
			Some(group) => group,
			None => {
				state.push(
					document,
					Box::new(Media {
						location: token.clone(),
						media: vec![],
					}),
				);

				document.last_element_mut::<Media>().unwrap()
			}
		};

		if let Err(err) = group.push(Box::new(Medium {
			location: token.clone(),
			reference: refname,
			uri,
			media_type,
			width,
			caption,
			description,
		})) {
			report_err!(
				&mut reports,
				token.source(),
				"Invalid Media".into(),
				span(token.range.clone(), err)
			);
		}

		reports
	}
}
