use std::collections::HashMap;

use super::compiler::CompiledDocument;
use super::compiler::Target;

#[derive(Debug)]
pub struct NavEntry {
	pub(crate) name: String,
	pub(crate) path: Option<String>,
	pub(crate) children: Option<HashMap<String, NavEntry>>,
}

#[derive(Debug)]
pub struct Navigation {
	pub(crate) entries: HashMap<String, NavEntry>,
}

impl Navigation {
	pub fn compile(&self, target: Target) -> String {
		let mut result = String::new();
		match target {
			Target::HTML => {
				result += r#"<ul id="navbar">"#;

				fn process(result: &mut String, name: &String, ent: &NavEntry, depth: usize) {
					let ent_path = ent
						.path
						.as_ref()
						.map_or("#".to_string(), |path| path.clone());
					result
						.push_str(format!(r#"<li><a href="{ent_path}">{name}</a></li>"#).as_str());

					if let Some(children) = ent.children.as_ref() {
						result.push_str("<ul>");
						for (name, ent) in children {
							process(result, name, ent, depth + 1);
						}
						result.push_str("</ul>");
					}
				}

				for (name, ent) in &self.entries {
					process(&mut result, name, ent, 0);
				}

				result += r#"</ul>"#;
			}
			_ => todo!(""),
		}
		result
	}
}

pub fn create_navigation(docs: &Vec<CompiledDocument>) -> Result<Navigation, String> {
	let mut nav = Navigation {
		entries: HashMap::new(),
	};

	for doc in docs {
		let cat = doc.get_variable("nav.category");
		let subcat = doc.get_variable("nav.subcategory");
		let title = doc
			.get_variable("nav.title")
			.or(doc.get_variable("doc.title"));
		let path = doc.get_variable("compiler.output");

		let (cat, title, path) = match (cat, title, path) {
			(Some(cat), Some(title), Some(path)) => (cat, title, path),
			_ => {
				println!("Skipping navigation generation for `{}`", doc.input);
				continue;
			}
		};

		if let Some(subcat) = subcat {
			// Get parent entry
			let mut pent = match nav.entries.get_mut(cat.as_str()) {
				Some(pent) => pent,
				None => {
					// Create parent entry
					nav.entries.insert(
						cat.clone(),
						NavEntry {
							name: cat.clone(),
							path: None,
							children: Some(HashMap::new()),
						},
					);
					nav.entries.get_mut(cat.as_str()).unwrap()
				}
			};

			// Insert into parent
			if let Some(previous) = pent.children.as_mut().unwrap().insert(
				subcat.clone(),
				NavEntry {
					name: subcat.clone(),
					path: Some(path.to_string()),
					children: None,
				},
			) {
				return Err(format!(
					"Duplicate subcategory:\n{subcat}\nclashes with:\n{previous:#?}"
				));
			}
		} else {
			// Get entry
			let mut ent = match nav.entries.get_mut(cat.as_str()) {
				Some(ent) => ent,
				None => {
					// Create parent entry
					nav.entries.insert(
						cat.clone(),
						NavEntry {
							name: cat.clone(),
							path: None,
							children: Some(HashMap::new()),
						},
					);
					nav.entries.get_mut(cat.as_str()).unwrap()
				}
			};

			if let Some(path) = ent.path.as_ref() {
				return Err(format!(
					"Duplicate category:\n{subcat:#?}\nwith previous path:\n{path}"
				));
			}
			ent.path = Some(path.to_string());
		}
	}

	Ok(nav)
}
