pub trait Element
{
    fn element_name(&self) -> &'static str;
    fn token(&'a self) -> Token<'a>
}

pub trait ReferenceableElement : Element
{
    fn reference_name(&self) -> Option<&String>;
}

pub struct Text
{
    content: String,
}

impl Text
{
    pub fn new<'h>(_content: &'h str) -> Text
    {
        Text {
            content: String::from(_content)
        }
    }
}

impl Element for Text
{
    fn element_name(&self) -> &'static str { "Text" }
}
