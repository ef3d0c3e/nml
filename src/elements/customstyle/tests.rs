use std::rc::Rc;

use crate::elements::paragraph::elem::Paragraph;
use crate::elements::raw::elem::Raw;
use crate::elements::text::elem::Text;
use crate::parser::langparser::LangParser;
use crate::parser::parser::ParseMode;
use crate::parser::parser::Parser;
use crate::parser::parser::ParserState;
use crate::parser::source::SourceFile;
use crate::validate_document;

#[test]
fn toggle() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
%<[main]
function my_style_start()
	nml.raw.push("inline", "start")
end
function my_style_end()
	nml.raw.push("inline", "end")
end
function red_style_start()
	nml.raw.push("inline", "<a style=\"color:red\">")
end
function red_style_end()
	nml.raw.push("inline", "</a>")
end
nml.custom_style.define_toggled("My Style", "|", my_style_start, my_style_end)
nml.custom_style.define_toggled("My Style2", "°", red_style_start, red_style_end)
>%
pre |styled| post °Hello°.
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

	validate_document!(doc.content().borrow(), 0,
		Paragraph {
			Text { content == "pre " };
			Raw { content == "start" };
			Text { content == "styled" };
			Raw { content == "end" };
			Text { content == " post " };
			Raw { content == "<a style=\"color:red\">" };
			Text { content == "Hello" };
			Raw { content == "</a>" };
			Text { content == "." };
		};
	);
}

#[test]
fn paired() {
	let source = Rc::new(SourceFile::with_content(
		"".to_string(),
		r#"
%<[main]
function my_style_start()
	nml.raw.push("inline", "start")
end
function my_style_end()
	nml.raw.push("inline", "end")
end
function red_style_start()
	nml.raw.push("inline", "<a style=\"color:red\">")
end
function red_style_end()
	nml.raw.push("inline", "</a>")
end
nml.custom_style.define_paired("My Style", "[", "]", my_style_start, my_style_end)
nml.custom_style.define_paired("My Style2", "(", ")", red_style_start, red_style_end)
>%
pre [styled] post (Hello).
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

	validate_document!(doc.content().borrow(), 0,
		Paragraph {
			Text { content == "pre " };
			Raw { content == "start" };
			Text { content == "styled" };
			Raw { content == "end" };
			Text { content == " post " };
			Raw { content == "<a style=\"color:red\">" };
			Text { content == "Hello" };
			Raw { content == "</a>" };
			Text { content == "." };
		};
	);
}
