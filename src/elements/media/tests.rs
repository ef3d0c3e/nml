use std::rc::Rc;

use crate::elements::media::elem::MediaType;
use crate::elements::media::elem::Medium;
use crate::elements::media::rule::MediaRule;
use crate::parser::langparser::LangParser;
use crate::parser::parser::ParseMode;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::rule::RegexRule;
use crate::parser::source::SourceFile;

#[test]
fn regex() {
	let rule = MediaRule::default();
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
	let (doc, _) = parser.parse(
		ParserState::new(&parser, None),
		source,
		None,
		ParseMode::default(),
	);

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
