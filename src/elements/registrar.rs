use crate::parser::parser::Parser;

use super::code::CodeRule;
use super::comment::CommentRule;
use super::graphviz::GraphRule;
use super::import::ImportRule;
use super::layout::LayoutRule;
use super::link::LinkRule;
use super::list::ListRule;
use super::media::MediaRule;
use super::paragraph::ParagraphRule;
use super::raw::RawRule;
use super::script::ScriptRule;
use super::section::SectionRule;
use super::style::StyleRule;
use super::tex::TexRule;
use super::text::TextRule;
use super::variable::VariableRule;
use super::variable::VariableSubstitutionRule;
use super::reference::ReferenceRule;

pub fn register<P: Parser>(parser: &mut P) {
	parser.add_rule(Box::new(CommentRule::new()), None).unwrap();
	parser.add_rule(Box::new(ParagraphRule::new()), None).unwrap();
	parser.add_rule(Box::new(ImportRule::new()), None).unwrap();
	parser.add_rule(Box::new(ScriptRule::new()), None).unwrap();
	parser.add_rule(Box::new(VariableRule::new()), None).unwrap();
	parser.add_rule(Box::new(VariableSubstitutionRule::new()), None).unwrap();
	parser.add_rule(Box::new(RawRule::new()), None).unwrap();
	parser.add_rule(Box::new(ListRule::new()), None).unwrap();
	parser.add_rule(Box::new(CodeRule::new()), None).unwrap();
	parser.add_rule(Box::new(TexRule::new()), None).unwrap();
	parser.add_rule(Box::new(GraphRule::new()), None).unwrap();
	parser.add_rule(Box::new(MediaRule::new()), None).unwrap();
	parser.add_rule(Box::new(LayoutRule::new()), None).unwrap();

	parser.add_rule(Box::new(StyleRule::new()), None).unwrap();
	parser.add_rule(Box::new(SectionRule::new()), None).unwrap();
	parser.add_rule(Box::new(LinkRule::new()), None).unwrap();
	parser.add_rule(Box::new(TextRule::default()), None).unwrap();
	parser.add_rule(Box::new(ReferenceRule::new()), None).unwrap();
}
