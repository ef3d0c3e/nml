use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use unicode_segmentation::UnicodeSegmentation;

use crate::document::document::Document;
use crate::document::document::DocumentAccessors;
use crate::document::element::ElemKind;
use crate::elements::paragraph::elem::Paragraph;

use super::scope::Scope;
use super::scope::ScopeAccessor;
use super::source::Source;
use super::source::Token;
use super::source::VirtualSource;
use super::state::ParseMode;
use super::translation::TranslationUnit;

/// Processes text for escape characters and paragraphing
/*
pub fn process_text(scope: Rc<RefCell<Scope>>, content: &str) -> String {
	let mut escaped = false;
	let mut newlines = 0usize; // Consecutive newlines
							//println!("Processing: [{content}]");
	let processed = content
		.graphemes(true)
		.fold((String::new(), None), |(mut out, prev), g| {
			if newlines != 0 && g != "\n" {
				newlines = 0;

				// Add a whitespace if necessary
				match out.chars().last() {
					Some(c) => {
						// NOTE: \n is considered whitespace, so previous codepoint can be \n
						// (Which can only be done by escaping it)
						if !c.is_whitespace() || c == '\n' {
							out += " ";
						}
					}
					None => {
						if scope.content_last()
							.and_then(|elem| {
								let Some(par) = elem.downcast_ref::<Paragraph>()
									else { return Some(false) };
								par.find_back(|e| e.kind() != ElemKind::Invisible)
									.map(|e| e.kind() == ElemKind::Inline)
							})
							.unwrap_or(false)
						{
							out += " ";
						}
					} // Don't output anything
				}
			}

			// Output grapheme literally when escaped
			if escaped {
				escaped = false;
				return (out + g, Some(g));
			}
			// Increment newlines counter
			else if g == "\n" {
				newlines += 1;
				return (out, Some(g));
			}
			// Determine if escaped
			else if g == "\\" {
				escaped = !escaped;
				return (out, Some(g));
			}
			// Whitespaces
			else if g.chars().count() == 1 && g.chars().last().unwrap().is_whitespace() {
				// Content begins with whitespace
				if prev.is_none() {
					if document.last_element::<Paragraph>().is_some() {
						return (out + g, Some(g));
					} else {
						return (out, Some(g));
					}
				}
				// Consecutive whitespaces are converted to a single whitespace
				else if prev.unwrap().chars().count() == 1
					&& prev.unwrap().chars().last().unwrap().is_whitespace()
				{
					return (out, Some(g));
				}
			}

			(out + g, Some(g))
		})
		.0
		.to_string();

	processed
}
*/

/// Transforms raw text content.
///
/// - Replaces all ascii whitespaces with SPACES
/// - Transforms consecutive whitespaces into a single whitespaces
/// - Escapes characters according to `\` usage
///
pub fn transform_text<S: AsRef<str>>(text: S) -> String {
	let mut escaped = false;
	text.as_ref()
		.chars()
		.skip_while(char::is_ascii_whitespace)
		.fold(
			(String::new(), Option::<char>::None),
			|(mut s, prev), ch| {
				if ch == '\\' {
					if escaped {
						escaped = false;
						return (s + "\\", Some(ch));
					}
					escaped = true;
				}

				if escaped {
					escaped = false;
					s.push(ch);
					return (s, Some(ch));
				}

				if ch.is_ascii_whitespace() {
					// Skip if previous is whitespace
					if prev.as_ref().is_some_and(char::is_ascii_whitespace) {
						return (s, Some(ch));
					}
					return (s + " ", Some(ch));
				}
				s.push(ch);
				(s, Some(ch))
			},
		)
		.0
	// TODO: Trailing spaces
}

/// Transforms source into a new [`VirtualSource`] using a `range`.
///
/// This function will extract the sub-source using the specified `range`, then escape `token` using the specified `escape` character.
/// It will also keep a list of removed/added characters and build an offset list that will be passed to the newly created source, via [`VirtualSource::new_offsets`].
///
///
/// # Notes
///
/// If you only need to escape content that won't be parsed, use [`escape_text`] instead.
pub fn escape_source(
	source: Arc<dyn Source>,
	range: Range<usize>,
	name: String,
	escape: char,
	token: &'static str,
) -> Arc<dyn Source> {
	let content = &source.content()[range.clone()];

	let mut processed = String::new();
	let mut escaped = 0;
	let mut token_it = token.chars().peekable();
	let mut offset = 0isize;
	let mut offsets: Vec<(usize, isize)> = vec![];
	for (pos, c) in content.chars().enumerate() {
		if c == escape {
			escaped += 1;
		} else if escaped % 2 == 1 && token_it.peek().map_or(false, |p| *p == c) {
			let _ = token_it.next();
			if token_it.peek().is_none() {
				(0..(escaped / 2)).for_each(|_| processed.push(escape));
				if (escaped + 1) / 2 != 0 {
					offset += (escaped + 1) / 2;
					offsets.push((pos - token.len() - escaped as usize / 2, offset));
				}
				escaped = 0;
				token_it = token.chars().peekable();
				processed.push_str(token);
			}
		} else {
			if escaped != 0 {
				// Add escapes
				(0..escaped).for_each(|_| processed.push('\\'));
				token_it = token.chars().peekable();
				escaped = 0;
			}
			processed.push(c);
		}
	}
	// Add trailing escapes
	(0..escaped).for_each(|_| processed.push('\\'));

	Arc::new(VirtualSource::new_offsets(
		Token::new(range, source),
		name,
		processed,
		offsets,
	))
}

/// Processed a string and escapes a single token out of it
/// Escaped characters other than the [`Token`] will be not be treated as escaped
///
/// # Example
/// ```
/// assert_eq!(scape_text('\\', "%", "escaped: \\%, also escaped: \\\\\\%, untouched: \\a"),
/// "escaped: %, also escaped: \\%, untouched: \\a");
/// ```
///
/// # Notes
///
/// If you need to create a source, do not use this function, use [`escape_source`] instead
/// as it will populate an offsets to get accurate diagnostics and semantics.
pub fn escape_text<S: AsRef<str>>(
	escape: char,
	token: &'static str,
	content: S,
	trim: bool,
) -> String {
	let mut processed = String::new();
	let mut escaped = 0;
	let mut token_it = token.chars().peekable();
	let data = if trim {
		content.as_ref().chars().as_str().trim_start().trim_end()
	} else {
		content.as_ref().chars().as_str()
	};
	for c in data.chars() {
		if c == escape {
			escaped += 1;
		} else if escaped % 2 == 1 && token_it.peek().map_or(false, |p| *p == c) {
			let _ = token_it.next();
			if token_it.peek().is_none() {
				(0..(escaped / 2)).for_each(|_| processed.push(escape));
				escaped = 0;
				token_it = token.chars().peekable();
				processed.push_str(token);
			}
		} else {
			if escaped != 0 {
				// Add untouched escapes
				(0..escaped).for_each(|_| processed.push('\\'));
				token_it = token.chars().peekable();
				escaped = 0;
			}
			processed.push(c);
		}
	}
	// Add trailing escapes
	(0..escaped / 2).for_each(|_| processed.push('\\'));

	processed
}

pub fn parse_paragraph<'u>(
	unit: &mut TranslationUnit<'u>,
	source: Arc<dyn Source>,
) -> Result<Rc<RefCell<Scope>>, String> {
	unit.with_child(
		source,
		ParseMode {
			paragraph_only: true,
		},
		false,
		|unit, scope| {
			// Parse into scope
			unit.parser.parse(unit);

			// Iterate over parsed content
			let mut iter = scope.content_iter();
			while let Some(elem) = iter.next() {
				// TODO
			}

			Ok(scope)
		},
	)
}

/// Parses source into a single paragraph
/// If source contains anything but a single paragraph, an error is returned
/*
pub fn parse_paragraph<'a>(
	state: &ParserState,
	source: Arc<dyn Source>,
	document: &'a dyn Document<'a>,
) -> Result<Box<Paragraph>, &'static str> {
	let parsed = state.with_state(|new_state| -> Box<dyn Document> {
		new_state
			.parser
			.parse(
				new_state,
				source.clone(),
				Some(document),
				ParseMode {
					paragraph_only: true,
				},
			)
			.0
	});
	if parsed.content().borrow().len() > 1 {
		return Err("Parsed document contains more than a single paragraph");
	} else if parsed.content().borrow().len() == 0 {
		return Err("Parsed document is empty");
	} else if parsed.last_element::<Paragraph>().is_none() {
		return Err("Parsed element is not a paragraph");
	} else if state.parser.has_error() {
		// FIXME: If parser had an error before, this wold trigger
		return Err("Parser error");
	}

	let paragraph = parsed.content().borrow_mut().pop().unwrap();
	Ok(paragraph.downcast::<Paragraph>().unwrap())
}
*/

#[cfg(test)]
mod tests {
	use super::*;
	use crate::document::element::ContainerElement;
	use crate::document::langdocument::LangDocument;
	use crate::elements::comment::elem::Comment;
	use crate::elements::style::elem::Style;
	use crate::elements::text::elem::Text;
	use crate::parser::source::SourceFile;
	use crate::parser::source::Token;

	#[test]
	fn process_text_tests() {
		let source = Arc::new(SourceFile::with_content(
			"".to_string(),
			"".to_string(),
			None,
		));
		let doc = LangDocument::new(source.clone(), None);

		assert_eq!(process_text(&doc, "a\nb"), "a b");
		assert_eq!(process_text(&doc, "a\n\nb"), "a b"); // Should never happen but why not
		assert_eq!(process_text(&doc, "a\\b"), "ab");
		assert_eq!(process_text(&doc, "a\\\nb"), "a\nb");
		assert_eq!(process_text(&doc, "a\\\\b"), "a\\b");
		assert_eq!(process_text(&doc, "a\\\\\nb"), "a\\ b");
		assert_eq!(process_text(&doc, "\na"), "a");

		let tok = Token::new(0..0, source);
		doc.push(Box::new(Paragraph {
			location: tok.clone(),
			content: Vec::new(),
		}));

		// Comments are ignored (kind => Invisible)
		(&doc as &dyn Document)
			.last_element_mut::<Paragraph>()
			.unwrap()
			.push(Box::new(Comment {
				location: tok.clone(),
				content: "COMMENT".into(),
			}))
			.unwrap();
		assert_eq!(process_text(&doc, "\na"), "a");

		// A space is appended as previous element is inline
		(&doc as &dyn Document)
			.last_element_mut::<Paragraph>()
			.unwrap()
			.push(Box::new(Text::new(tok.clone(), "TEXT".to_string())))
			.unwrap();
		assert_eq!(process_text(&doc, "\na"), " a");

		(&doc as &dyn Document)
			.last_element_mut::<Paragraph>()
			.unwrap()
			.push(Box::new(Style {
				location: tok.clone(),
				kind: 0,
				close: false,
			}))
			.unwrap();
		assert_eq!(process_text(&doc, "\na"), " a");
	}

	#[test]
	fn process_escaped_tests() {
		assert_eq!(
			escape_text(
				'\\',
				"%",
				"escaped: \\%, also escaped: \\\\\\%, untouched: \\a",
				true
			),
			"escaped: %, also escaped: \\%, untouched: \\a"
		);
		assert_eq!(
			escape_text('"', "><)))°>", "Escaped fish: \"><)))°>", false),
			"Escaped fish: ><)))°>".to_string()
		);
		assert_eq!(
			escape_text('\\', "]", "Escaped \\]", true),
			"Escaped ]".to_string()
		);
		assert_eq!(
			escape_text('\\', "]", "Unescaped \\\\]", false),
			"Unescaped \\\\]".to_string()
		);
		assert_eq!(
			escape_text('\\', "]", "Escaped \\\\\\]", true),
			"Escaped \\]".to_string()
		);
		assert_eq!(
			escape_text('\\', "]", "Unescaped \\\\\\\\]", false),
			"Unescaped \\\\\\\\]".to_string()
		);
		assert_eq!(escape_text('\\', ")", "A\\)B\\", true), "A)B".to_string(),);
		assert_eq!(
			escape_text('\\', ")", "A\\)B\\\\", false),
			"A)B\\".to_string(),
		);
	}
}
