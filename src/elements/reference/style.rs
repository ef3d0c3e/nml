use serde::Deserialize;
use serde::Serialize;

use crate::impl_elementstyle;

#[derive(Debug, Serialize, Deserialize)]
#[auto_registry::auto_registry(registry = "elem_styles")]
pub struct ExternalReferenceStyle {
	pub format_unspecific: String,
	pub format_specific: String,
}

impl Default for ExternalReferenceStyle {
	fn default() -> Self {
		Self {
			format_unspecific: "(#{refname})".into(),
			format_specific: "({refdoc}#{refname})".into(),
		}
	}
}

impl_elementstyle!(ExternalReferenceStyle, "style.external_reference");
