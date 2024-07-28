use std::collections::HashMap;

use crate::document::document::Document;

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

pub fn create_navigation(docs: &Vec<Box<dyn Document>>) -> Result<Navigation, String> {
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
				println!(
					"Skipping navigation generation for `{}`",
					doc.source().name()
				);
				continue;
			}
		};

		if let Some(subcat) = subcat {
			// Get parent entry
			let cat_name = cat.to_string();
			let mut pent = match nav.entries.get_mut(cat_name.as_str()) {
				Some(pent) => pent,
				None => {
					// Create parent entry
					nav.entries.insert(
						cat_name.clone(),
						NavEntry {
							name: cat_name.clone(),
							path: None,
							children: Some(HashMap::new()),
						},
					);
					nav.entries.get_mut(cat_name.as_str()).unwrap()
				}
			};

			// Insert into parent
			let subcat_name = subcat.to_string();
			if let Some(previous) = pent.children.as_mut().unwrap().insert(
				subcat_name.clone(),
				NavEntry {
					name: subcat_name.clone(),
					path: Some(path.to_string()),
					children: None,
				},
			) {
				return Err(format!(
					"Duplicate subcategory:\n{subcat:#?}\nclashes with:\n{previous:#?}"
				));
			}
		} else {
			// Get entry
			let cat_name = cat.to_string();
			let mut ent = match nav.entries.get_mut(cat_name.as_str()) {
				Some(ent) => ent,
				None => {
					// Create parent entry
					nav.entries.insert(
						cat_name.clone(),
						NavEntry {
							name: cat_name.clone(),
							path: None,
							children: Some(HashMap::new()),
						},
					);
					nav.entries.get_mut(cat_name.as_str()).unwrap()
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
