use crate::parser::parser::Parser;

use super::{code::CodeRule, comment::CommentRule, import::ImportRule, link::LinkRule, list::ListRule, paragraph::ParagraphRule, raw::RawRule, script::ScriptRule, section::SectionRule, style::StyleRule, tex::TexRule, variable::{VariableRule, VariableSubstitutionRule}};


pub fn register<P: Parser>(parser: &mut P)
{
	parser.add_rule(Box::new(CommentRule::new()), None);
	parser.add_rule(Box::new(ParagraphRule::new()), None);
    parser.add_rule(Box::new(ImportRule::new()), None);
    parser.add_rule(Box::new(ScriptRule::new()), None);
    parser.add_rule(Box::new(VariableRule::new()), None);
    parser.add_rule(Box::new(VariableSubstitutionRule::new()), None);
    parser.add_rule(Box::new(RawRule::new()), None);
    parser.add_rule(Box::new(ListRule::new()), None);
    parser.add_rule(Box::new(CodeRule::new()), None);
    parser.add_rule(Box::new(TexRule::new()), None);

    parser.add_rule(Box::new(StyleRule::new()), None);
    parser.add_rule(Box::new(SectionRule::new()), None);
    parser.add_rule(Box::new(LinkRule::new()), None);
}
