use serde::Deserialize;
use serde::Serialize;

use crate::impl_elementstyle;

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub enum SectionLinkPos {
	Before,
	After,
	None,
}

#[derive(Debug, Serialize, Deserialize)]
#[auto_registry::auto_registry(registry = "elem_styles")]
pub struct SectionStyle {
	pub link_pos: SectionLinkPos,
	pub link: [String; 3],
}

impl Default for SectionStyle {
	fn default() -> Self {
		Self {
			link_pos: SectionLinkPos::Before,
			link: ["".into(), "🔗".into(), " ".into()],
		}
	}
}

impl_elementstyle!(SectionStyle, "style.section");
