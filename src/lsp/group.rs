

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ConcealInfo {
	pub range: tower_lsp::lsp_types::Range,
	pub conceal_text: ConcealTarget,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ConcealTarget {
	Text(String),
	Highlight {
		text: String,
		highlight_group: String,
	},
}
