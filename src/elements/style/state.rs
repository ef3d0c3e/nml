use std::collections::{HashMap, HashSet};

use regex::Regex;

use crate::{parser::{source::Token, state::CustomState}, unit::translation::{CustomData, TranslationUnit}};

pub struct Style
{
	/// Style name
	pub(crate) name: String,
	/// Style enable regex
	pub(crate) start_re: Regex,
	/// Style disable regex
	pub(crate) end_re: Regex,
}


pub static STYLE_STATE: &str = "nml.style.state";

/// State for styles
#[derive(Debug, Default)]
pub struct StyleState
{
	/// Enabled styles and their enabled location
	pub(crate) enabled: Vec<(String, Token)>,
}

impl CustomState for StyleState {
    fn name(&self) -> &str {
        STYLE_STATE
    }
}

pub static STYLE_CUSTOM: &str = "nml.style.registered";
/// Data for styles
pub struct StyleData
{
	/// All registered styles
	pub(crate) registered: HashMap<String, Style>,
}

impl CustomData for StyleData
{
    fn name(&self) -> &str {
        STYLE_CUSTOM
    }
}
