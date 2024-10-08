use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::str::FromStr;

use ariadne::Fmt;
use ariadne::Label;
use ariadne::Report;
use ariadne::ReportKind;
use regex::Captures;
use regex::Match;
use regex::Regex;
use regex::RegexBuilder;

use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::document::DocumentAccessors;
use crate::document::element::ContainerElement;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::document::element::ReferenceableElement;
use crate::document::references::validate_refname;
use crate::parser::parser::ParserState;
use crate::parser::parser::ReportColors;
use crate::parser::rule::RegexRule;
use crate::parser::source::Source;
use crate::parser::source::Token;
use crate::parser::source::VirtualSource;
use crate::parser::util;
use crate::parser::util::parse_paragraph;
use crate::parser::util::Property;
use crate::parser::util::PropertyMap;
use crate::parser::util::PropertyMapError;
use crate::parser::util::PropertyParser;

use super::paragraph::Paragraph;
use super::reference::InternalReference;

#[derive(Debug, PartialEq, Eq)]
pub enum MediaType {
	IMAGE,
	VIDEO,
	AUDIO,
}

impl FromStr for MediaType {
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s {
			"image" => Ok(MediaType::IMAGE),
			"video" => Ok(MediaType::VIDEO),
			"audio" => Ok(MediaType::AUDIO),
			_ => Err(format!("Unknown media type: {s}")),
		}
	}
}

#[derive(Debug)]
struct Media {
	pub(self) location: Token,
	pub(self) media: Vec<Box<dyn Element>>,
}

impl Element for Media {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Block }

	fn element_name(&self) -> &'static str { "Media" }

	fn as_container(&self) -> Option<&dyn ContainerElement> { Some(self) }

	fn compile(&self, compiler: &Compiler, document: &dyn Document, cursor: usize) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				let mut result = String::new();

				result.push_str("<div class=\"media\">");
				for medium in &self.media {
					result += medium.compile(compiler, document, cursor+result.len())?.as_str();
				}
				result.push_str("</div>");

				Ok(result)
			}
			_ => todo!(""),
		}
	}
}

impl ContainerElement for Media {
	fn contained(&self) -> &Vec<Box<dyn Element>> { &self.media }

	fn push(&mut self, elem: Box<dyn Element>) -> Result<(), String> {
		let medium = match elem.downcast_ref::<Medium>() {
			Some(medium) => medium,
			None => return Err("Attempted to insert invalid element into Media".to_string()),
		};
		if self.location.source() != medium.location.source() {
			return Err(format!(
				"Attempted to insert medium from {} into medium from {}",
				self.location.source(),
				medium.location.source()
			));
		}

		self.location.range = self.location.start()..medium.location.end();
		self.media.push(elem);
		Ok(())
	}
}

#[derive(Debug)]
struct Medium {
	pub(self) location: Token,
	pub(self) reference: String,
	pub(self) uri: String,
	pub(self) media_type: MediaType,
	pub(self) width: Option<String>,
	pub(self) caption: Option<String>,
	pub(self) description: Option<Paragraph>,
}

impl Element for Medium {
	fn location(&self) -> &Token { &self.location }

	fn kind(&self) -> ElemKind { ElemKind::Block }

	fn element_name(&self) -> &'static str { "Medium" }

	fn as_referenceable(&self) -> Option<&dyn ReferenceableElement> { Some(self) }

	fn compile(&self, compiler: &Compiler, document: &dyn Document, cursor: usize) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				let mut result = String::new();

				// Reference
				let elemref = document.get_reference(self.reference.as_str()).unwrap();
				let refcount = compiler.reference_id(document, elemref);

				let width = self
					.width
					.as_ref()
					.map_or(String::new(), |w| format!(r#" style="width:{w};""#));
				result.push_str(format!(r#"<div id="{}" class="medium"{width}>"#, self.refid(compiler, refcount)).as_str());
				result += match self.media_type {
					MediaType::IMAGE => format!(r#"<a href="{0}"><img src="{0}"></a>"#, self.uri),
					MediaType::VIDEO => format!(r#"<video controls{width}><source src="{0}"></video>"#, self.uri
					),
					MediaType::AUDIO => {
						format!(r#"<audio controls src="{0}"{width}></audio>"#, self.uri)
					}
				}
				.as_str();

				let caption = self
					.caption
					.as_ref().map(|cap| format!(
							" {}",
							Compiler::sanitize(compiler.target(), cap.as_str())
						))
					.unwrap_or_default();

				result.push_str(
					format!(r#"<p class="medium-refname">({refcount}){caption}</p>"#).as_str(),
				);
				if let Some(paragraph) = self.description.as_ref() {
					result += paragraph.compile(compiler, document, cursor+result.len())?.as_str();
				}
				result.push_str("</div>");

				Ok(result)
			}
			_ => todo!(""),
		}
	}
}

impl ReferenceableElement for Medium {
	fn reference_name(&self) -> Option<&String> { Some(&self.reference) }

	fn refcount_key(&self) -> &'static str { "medium" }

	fn compile_reference(
		&self,
		compiler: &Compiler,
		_document: &dyn Document,
		reference: &InternalReference,
		refid: usize,
	) -> Result<String, String> {
		match compiler.target() {
			Target::HTML => {
				let caption = reference
					.caption()
					.map_or(format!("({refid})"), |cap| cap.clone());

				// TODO Handle other kind of media
				match self.media_type {
					MediaType::IMAGE => Ok(format!(
						"<a class=\"medium-ref\" href=\"#medium-{refid}\">{caption}<img src=\"{}\"></a>",
						self.uri
					)),
					MediaType::VIDEO => Ok(format!(
						"<a class=\"medium-ref\" href=\"#medium-{refid}\">{caption}<video><source src=\"{0}\"></video></a>",
						self.uri
					)),
					_ => todo!(""),
				}
			}
			_ => todo!(""),
		}
	}

	fn refid(&self, _compiler: &Compiler, refid: usize) -> String {
		format!("medium-{refid}")
	}
}

#[auto_registry::auto_registry(registry = "rules", path = "crate::elements::media")]
pub struct MediaRule {
	re: [Regex; 1],
	properties: PropertyParser,
}

impl MediaRule {
	pub fn new() -> Self {
		let mut props = HashMap::new();
		props.insert(
			"type".to_string(),
			Property::new(
				false,
				"Override for the media type detection".to_string(),
				None,
			),
		);
		props.insert(
			"width".to_string(),
			Property::new(false, "Override for the media width".to_string(), None),
		);
		props.insert(
			"caption".to_string(),
			Property::new(false, "Medium caption".to_string(), None),
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

	fn validate_uri(uri: &str) -> Result<&str, String> {
		let trimmed = uri.trim_start().trim_end();

		if trimmed.is_empty() {
			return Err("URIs is empty".to_string());
		}

		Ok(trimmed)
	}

	fn parse_properties(
		&self,
		colors: &ReportColors,
		token: &Token,
		m: &Option<Match>,
	) -> Result<PropertyMap, Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		match m {
			None => match self.properties.default() {
				Ok(properties) => Ok(properties),
				Err(e) => Err(
					Report::build(ReportKind::Error, token.source(), token.start())
						.with_message("Invalid Media Properties")
						.with_label(
							Label::new((token.source().clone(), token.range.clone()))
								.with_message(format!("Media is missing required property: {e}"))
								.with_color(colors.error),
						)
						.finish(),
				),
			},
			Some(props) => {
				let processed =
					util::process_escaped('\\', "]", props.as_str().trim_start().trim_end());
				match self.properties.parse(processed.as_str()) {
					Err(e) => Err(
						Report::build(ReportKind::Error, token.source(), props.start())
							.with_message("Invalid Media Properties")
							.with_label(
								Label::new((token.source().clone(), props.range()))
									.with_message(e)
									.with_color(colors.error),
							)
							.finish(),
					),
					Ok(properties) => Ok(properties),
				}
			}
		}
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
}

impl RegexRule for MediaRule {
	fn name(&self) -> &'static str { "Media" }
	fn previous(&self) -> Option<&'static str> { Some("Graphviz") }

	fn regexes(&self) -> &[regex::Regex] { &self.re }

	fn on_regex_match<'a>(
		&self,
		_: usize,
		state: &ParserState,
		document: &'a (dyn Document<'a> + 'a),
		token: Token,
		matches: Captures,
	) -> Vec<Report<'_, (Rc<dyn Source>, Range<usize>)>> {
		let mut reports = vec![];

		let refname = match (
			matches.get(1).unwrap(),
			validate_refname(document, matches.get(1).unwrap().as_str(), true),
		) {
			(_, Ok(refname)) => refname.to_string(),
			(m, Err(err)) => {
				reports.push(
					Report::build(ReportKind::Error, token.source(), m.start())
						.with_message("Invalid Media Refname")
						.with_label(
							Label::new((token.source().clone(), m.range())).with_message(err),
						)
						.finish(),
				);
				return reports;
			}
		};

		let uri = match (
			matches.get(2).unwrap(),
			MediaRule::validate_uri(matches.get(2).unwrap().as_str()),
		) {
			(_, Ok(uri)) => util::process_escaped('\\', ")", uri),
			(m, Err(err)) => {
				reports.push(
					Report::build(ReportKind::Error, token.source(), m.start())
						.with_message("Invalid Media URI")
						.with_label(
							Label::new((token.source().clone(), m.range())).with_message(err),
						)
						.finish(),
				);
				return reports;
			}
		};

		// Properties
		let properties = match self.parse_properties(state.parser.colors(), &token, &matches.get(3))
		{
			Ok(pm) => pm,
			Err(report) => {
				reports.push(report);
				return reports;
			}
		};

		let media_type =
			match Self::detect_filetype(uri.as_str()) {
				Some(media_type) => media_type,
				None => match properties.get("type", |prop, value| {
					MediaType::from_str(value.as_str()).map_err(|e| (prop, e))
				}) {
					Ok((_prop, kind)) => kind,
					Err(e) => match e {
						PropertyMapError::ParseError((prop, err)) => {
							reports.push(
								Report::build(ReportKind::Error, token.source(), token.start())
									.with_message("Invalid Media Property")
									.with_label(
										Label::new((token.source().clone(), token.range.clone()))
											.with_message(format!(
												"Property `type: {}` cannot be converted: {}",
												prop.fg(state.parser.colors().info),
												err.fg(state.parser.colors().error)
											))
											.with_color(state.parser.colors().warning),
									)
									.finish(),
							);
							return reports;
						}
						PropertyMapError::NotFoundError(err) => {
							reports.push(
							Report::build(ReportKind::Error, token.source(), token.start())
							.with_message("Invalid Media Property")
							.with_label(
								Label::new((
										token.source().clone(),
										token.start() + 1..token.end(),
								))
								.with_message(format!("{err}. Required because mediatype could not be detected"))
								.with_color(state.parser.colors().error),
							)
							.finish(),
						);
							return reports;
						}
					},
				},
			};

		let width = properties
			.get("width", |_, value| -> Result<String, ()> {
				Ok(value.clone())
			})
			.ok().map(|(_, s)| s);

		let caption = properties
			.get("caption", |_, value| -> Result<String, ()> {
				Ok(value.clone())
			})
			.ok().map(|(_, value)| value);

		let description = match matches.get(4) {
			Some(content) => {
				let source = Rc::new(VirtualSource::new(
					Token::new(content.range(), token.source()),
					format!("Media[{refname}] description"),
					content.as_str().trim_start().trim_end().to_string(),
				));
				if source.content().is_empty() {
					None
				} else {
					match parse_paragraph(state, source, document) {
						Ok(paragraph) => Some(*paragraph),
						Err(err) => {
							reports.push(
								Report::build(ReportKind::Error, token.source(), content.start())
									.with_message("Invalid Media Description")
									.with_label(
										Label::new((token.source().clone(), content.range()))
											.with_message(format!(
												"Could not parse description: {err}"
											))
											.with_color(state.parser.colors().error),
									)
									.finish(),
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
			reports.push(
				Report::build(ReportKind::Error, token.source(), token.start())
					.with_message("Invalid Media")
					.with_label(
						Label::new((token.source().clone(), token.range.clone()))
							.with_message(err)
							.with_color(state.parser.colors().error),
					)
					.finish(),
			);
		}

		reports
	}
}

#[cfg(test)]
mod tests {
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;

	use super::*;

	#[test]
	fn regex() {
		let rule = MediaRule::new();
		let re = &rule.regexes()[0];

		assert!(re.is_match("![refname](some path...)[some properties] some description"));
		assert!(re.is_match(
			r"![refname](some p\)ath...\\)[some propert\]ies\\\\] some description\\nanother line"
		));
		assert!(re.is_match_at("![r1](uri1)[props1] desc1\n![r2](uri2)[props2] desc2", 26));
	}

	#[test]
	fn element_test() {
		let source = Rc::new(SourceFile::with_content(
			"".to_string(),
			r#"
![ref1](  image.png )[width = 200px, caption = Caption\,] Description
![ref2]( ur\)i\\)[type=audio]
				"#
			.to_string(),
			None,
		));
		let parser = LangParser::default();
		let (doc, _) = parser.parse(ParserState::new(&parser, None), source, None);

		let borrow = doc.content().borrow();
		let group = borrow.first().as_ref().unwrap().as_container().unwrap();

		let first = group.contained()[0].downcast_ref::<Medium>().unwrap();
		assert_eq!(first.reference, "ref1");
		assert_eq!(first.uri, "image.png");
		assert_eq!(first.media_type, MediaType::IMAGE);
		assert_eq!(first.width, Some("200px".to_string()));
		assert_eq!(first.caption, Some("Caption,".to_string()));

		let second = group.contained()[1].downcast_ref::<Medium>().unwrap();
		assert_eq!(second.reference, "ref2");
		assert_eq!(second.uri, "ur)i\\");
		assert_eq!(second.media_type, MediaType::AUDIO);
	}
}
