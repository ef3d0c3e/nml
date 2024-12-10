use crate::compiler::compiler::Compiler;
use crate::compiler::compiler::Target;
use crate::document::document::Document;
use crate::document::element::ElemKind;
use crate::document::element::Element;
use crate::lsp::semantic::Semantics;
use crate::lua::kernel::CTX;
use crate::parser::parser::ParseMode;
use crate::parser::parser::ParserState;
use crate::parser::rule::RegexRule;
use crate::parser::source::Token;
use crate::parser::state::RuleState;
use crate::parser::state::Scope;
use ariadne::Fmt;
use lsp::conceal::ConcealTarget;
use lsp::conceal::Conceals;
use lsp::styles::Styles;
use mlua::Function;
use regex::Captures;
use regex::Regex;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;



#[cfg(test)]
mod tests {
	use elements::paragraph::elem::Paragraph;

	use crate::elements::text::Text;
	use crate::parser::langparser::LangParser;
	use crate::parser::parser::Parser;
	use crate::parser::source::SourceFile;
	use crate::validate_document;
	use crate::validate_semantics;

	use super::*;

}
