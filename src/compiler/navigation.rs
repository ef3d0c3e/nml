use std::collections::HashMap;

use crate::compiler::compiler::Compiler;

use super::compiler::CompiledDocument;
use super::compiler::Target;

#[derive(Debug, Default)]
pub struct NavEntry {
	pub(self) entries: Vec<(String, String)>,
	pub(self) children: HashMap<String, NavEntry>,
}

impl NavEntry {
	// FIXME: Sanitize
	pub fn compile(&self, target: Target, doc: &CompiledDocument) -> String {
		let categories = vec![
			doc.get_variable("nav.category").map_or("", |s| s.as_str()),
			doc.get_variable("nav.subcategory")
				.map_or("", |s| s.as_str()),
		];

		let mut result = String::new();
		match target {
			Target::HTML => {
				result += r#"<div class="navbar"><ul>"#;

				fn process(
					target: Target,
					categories: &Vec<&str>,
					did_match: bool,
					result: &mut String,
					entry: &NavEntry,
					depth: usize,
				) {
					// Orphans = Links
					for (title, path) in &entry.entries {
						result.push_str(
							format!(
								r#"<li><a href="{}">{}</a></li>"#,
								Compiler::sanitize(target, path),
								Compiler::sanitize(target, title)
							)
							.as_str(),
						);
					}

					// Recurse
					for (name, ent) in &entry.children {
						let is_match = if did_match {
							categories.get(depth) == Some(&name.as_str())
						} else {
							false || depth == 0
						};
						result.push_str("<li>");
						result.push_str(
							format!(
								"<details{}><summary>{}</summary>",
								["", " open"][is_match as usize],
								Compiler::sanitize(target, name)
							)
							.as_str(),
						);
						result.push_str("<ul>");
						process(target, categories, is_match, result, ent, depth + 1);
						result.push_str("</ul></details></li>");
					}
				}

				process(target, &categories, true, &mut result, self, 0);

				result += r#"</ul></div>"#;
			}
			_ => todo!(""),
		}
		result
	}
}

pub fn create_navigation(docs: &Vec<CompiledDocument>) -> Result<NavEntry, String> {
	let mut nav = NavEntry {
		entries: vec![],
		children: HashMap::new(),
	};

	for doc in docs {
		let cat = doc.get_variable("nav.category");
		let subcat = doc.get_variable("nav.subcategory");
		let title = doc
			.get_variable("nav.title")
			.or(doc.get_variable("doc.title"));
		let path = doc.get_variable("compiler.output");

		let (title, path) = match (title, path) {
			(Some(title), Some(path)) => (title, path),
			_ => {
				eprintln!("Skipping navigation generation for `{}`, must have a defined `@nav.title` and `@compiler.output`", doc.input);
				continue;
			}
		};

		// Get entry to insert into
		let pent = if let Some(subcat) = subcat {
			let cat = match cat {
				Some(cat) => cat,
				None => {
					eprintln!(
						"Skipping `{}`: No `@nav.category`, but `@nav.subcategory` is set",
						doc.input
					);
					continue;
				}
			};

			let cat_ent = match nav.children.get_mut(cat.as_str()) {
				Some(cat_ent) => cat_ent,
				None => {
					// Insert
					nav.children.insert(cat.clone(), NavEntry::default());
					nav.children.get_mut(cat.as_str()).unwrap()
				}
			};

			match cat_ent.children.get_mut(subcat.as_str()) {
				Some(subcat_ent) => subcat_ent,
				None => {
					// Insert
					cat_ent.children.insert(subcat.clone(), NavEntry::default());
					cat_ent.children.get_mut(subcat.as_str()).unwrap()
				}
			}
		} else if let Some(cat) = cat {
			match nav.children.get_mut(cat.as_str()) {
				Some(cat_ent) => cat_ent,
				None => {
					// Insert
					nav.children.insert(cat.clone(), NavEntry::default());
					nav.children.get_mut(cat.as_str()).unwrap()
				}
			}
		} else {
			&mut nav
		};

		pent.entries.push((title.clone(), path.clone()))
	}

	Ok(nav)
}
