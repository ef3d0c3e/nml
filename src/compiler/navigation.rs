use std::cell::RefCell;
use std::collections::HashMap;

use crate::compiler::compiler::Compiler;

use super::compiler::CompiledDocument;
use super::compiler::Target;
use super::postprocess::PostProcess;

#[derive(Debug, Default)]
pub struct NavEntry {
	pub(self) entries: Vec<(String, String, Option<String>)>,
	pub(self) children: HashMap<String, NavEntry>,
}

impl NavEntry {
	// FIXME: Sanitize
	pub fn compile(&self, target: Target, doc: &RefCell<CompiledDocument>) -> String {
		let doc_borrow = doc.borrow();
		let categories = vec![
			doc_borrow
				.get_variable("nav.category")
				.map_or("", |s| s.as_str()),
			doc_borrow
				.get_variable("nav.subcategory")
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
					for (title, path, _) in &entry.entries {
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

	fn sort_entry(
		left: &(String, String, Option<String>),
		right: &(String, String, Option<String>),
	) -> std::cmp::Ordering {
		match (&left.2, &right.2) {
			(Some(_), Some(_)) => left.0.cmp(&right.0),
			(Some(lp), None) => {
				if &right.0 == lp {
					std::cmp::Ordering::Greater
				} else {
					left.0.cmp(&right.0)
				}
			}
			(None, Some(rp)) => {
				if &left.0 == rp {
					std::cmp::Ordering::Less
				} else {
					left.0.cmp(&right.0)
				}
			}
			(None, None) => left.0.cmp(&right.0),
		}
	}
}

pub fn create_navigation(
	docs: &Vec<(RefCell<CompiledDocument>, Option<PostProcess>)>,
) -> Result<NavEntry, String> {
	let mut nav = NavEntry {
		entries: vec![],
		children: HashMap::new(),
	};

	// All paths (for duplicate checking)
	let mut all_paths = HashMap::new();

	for (doc, _) in docs {
		let doc_borrow = doc.borrow();
		let cat = doc_borrow.get_variable("nav.category");
		let subcat = doc_borrow.get_variable("nav.subcategory");
		let title = doc_borrow
			.get_variable("nav.title")
			.or(doc_borrow.get_variable("doc.title"));
		let previous = doc_borrow.get_variable("nav.previous").map(|s| s.clone());
		let path = doc_borrow.get_variable("compiler.output");

		let (title, path) = match (title, path) {
			(Some(title), Some(path)) => (title, path),
			_ => {
				eprintln!("Skipping navigation generation for `{}`, must have a defined `@nav.title` and `@compiler.output`", doc_borrow.input);
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
						doc_borrow.input
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

		// Find duplicates titles in current parent
		for (ent_title, _, _) in &pent.entries {
			if ent_title == title {
				return Err(format!(
					"Conflicting entry title `{title}` for entries with the same parent: ({})",
					pent.entries
						.iter()
						.map(|(title, _, _)| title.clone())
						.collect::<Vec<_>>()
						.join(", ")
				));
			}
		}

		// Find duplicate paths
		if let Some(dup_title) = all_paths.get(path) {
			return Err(format!("Conflicting paths: `{path}`. Previously used for entry: `{dup_title}`, conflicting use in `{title}`"));
		}
		all_paths.insert(path.clone(), title.clone());

		pent.entries.push((title.clone(), path.clone(), previous));
	}

	// Sort entries
	fn sort_entries(nav: &mut NavEntry) {
		nav.entries
			.sort_unstable_by(|l, r| NavEntry::sort_entry(l, r));

		for (_, child) in &mut nav.children {
			sort_entries(child);
		}
	}
	sort_entries(&mut nav);

	Ok(nav)
}

#[cfg(test)]
mod tests {
	use rand::rngs::OsRng;
	use rand::RngCore;

	use crate::compiler::process::process_from_memory;

	use super::*;

	#[test]
	fn sort() {
		let entries: Vec<(String, String, Option<String>)> = vec![
			("Index".into(), "".into(), None),
			("AB".into(), "".into(), Some("Index".into())),
			("Getting Started".into(), "".into(), Some("Index".into())),
			("Sections".into(), "".into(), Some("Getting Started".into())),
			("Style".into(), "".into(), Some("Getting Started".into())),
		];
		let mut shuffled = entries.clone();
		for _ in 0..10 {
			for i in 0..5 {
				let pos = OsRng.next_u64() % entries.len() as u64;
				shuffled.swap(i, pos as usize);
			}

			shuffled.sort_by(|l, r| NavEntry::sort_entry(l, r));

			assert_eq!(shuffled, entries);
		}
	}

	#[test]
	pub fn batch() {
		let result = process_from_memory(
			Target::HTML,
			vec![
				r#"
@html.page_title = 0
@compiler.output = 0.html
@nav.title = C
@nav.category = First
"#
				.into(),
				r#"
@html.page_title = 1
@compiler.output = 1.html
@nav.title = A
@nav.category = First
"#
				.into(),
				r#"
@html.page_title = 2
@compiler.output = 2.html
@nav.title = B
@nav.category = First
"#
				.into(),
			],
		)
		.unwrap();

		let nav = create_navigation(&result).unwrap();
		assert_eq!(
			nav.children.get("First").unwrap().entries,
			vec![
				("A".to_string(), "1.html".to_string(), None,),
				("B".to_string(), "2.html".to_string(), None,),
				("C".to_string(), "0.html".to_string(), None,),
			]
		);
	}
}
