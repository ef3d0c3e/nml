use std::str::FromStr;

use downcast_rs::{impl_downcast, Downcast};
use crate::{compiler::compiler::Compiler, parser::source::Token};

use super::document::Document;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ElemKind {
    /// An invisible element (i.e comment)
    Invisible,
    /// Special elements don't trigger special formatting events
    Special,
    /// Inline elements don't break paragraphing
    Inline,
    /// Block elements are outside of paragraphs
    Block,
}

impl FromStr for ElemKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s
		{
			"invisible" => Ok(ElemKind::Invisible),
			"special"   => Ok(ElemKind::Special),
			"inline"    => Ok(ElemKind::Inline),
			"block"     => Ok(ElemKind::Block),
			_ => Err(format!("Unknown ElemKind: {s}"))
		}
    }
}

pub trait Element: Downcast
{
    /// Gets the element defined location i.e token without filename
    fn location(&self) -> &Token;

    fn kind(&self) -> ElemKind;

    /// Get the element's name
	fn element_name(&self) -> &'static str;

    /// Outputs element to string for debug purposes
    fn to_string(&self) -> String;

    fn as_referenceable(&self) -> Option<&dyn ReferenceableElement> { None }

    /// Compiles element
    fn compile(&self, compiler: &Compiler, document: &dyn Document) -> Result<String, String>;
}
impl_downcast!(Element);

impl core::fmt::Debug for dyn Element
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

pub trait ReferenceableElement : Element {
    /// Reference name
	fn reference_name(&self) -> Option<&String>;
}
