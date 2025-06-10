/*
use super::compiler::Target;
use super::sanitize::Sanitizer;

#[derive(Debug, Default, PartialEq, Eq, Clone)]
pub struct NavEntry {
	title: String,
	path: String,
	previous: Option<String>,
}

#[derive(Debug, Default)]
pub struct NavEntries {
	pub(self) entries: Vec<NavEntry>,
	pub(self) children: HashMap<String, NavEntries>,
}

impl NavEntries {
	// FIXME: Sanitize
	pub fn compile(&self, sanitizer: Sanitizer, doc: &RefCell<CompiledDocument>) -> String {
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
		match sanitizer.target() {
			Target::HTML => {
				result += r#"<input id="navbar-checkbox" class="toggle" type="checkbox" style="display:none" checked><div id="navbar"><ul>"#;

				fn process(
					sanitizer: Sanitizer,
					doc_path: &String,
					categories: &Vec<&str>,
					did_match: bool,
					result: &mut String,
					entry: &NavEntries,
					depth: usize,
				) {
					// Orphans = Links
					for entry in &entry.entries {
						let style = if doc_path == &entry.path {
							" class=\"navbar-entry-current\""
						} else {
							" class=\"navbar-entry\""
						}
						.to_string();
						result.push_str(
							format!(
								r#"<li {style}><a href="{}">{}</a></li>"#,
								sanitizer.sanitize(entry.path.as_str()),
								sanitizer.sanitize(entry.title.as_str())
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
								"<details{}><summary class=\"navbar-category\">{}</summary>",
								["", " open"][is_match as usize],
								sanitizer.sanitize(name)
							)
							.as_str(),
						);
						result.push_str("<ul>");
						process(
							sanitizer,
							doc_path,
							categories,
							is_match,
							result,
							ent,
							depth + 1,
						);
						result.push_str("</ul></details></li>");
					}
				}

				process(
					sanitizer,
					doc_borrow
						.get_variable("compiler.output")
						.unwrap_or(&String::new()),
					&categories,
					true,
					&mut result,
					self,
					0,
				);

				result += r#"</ul></div><label for="navbar-checkbox" class="navbar-checkbox-label">&#9776;</label>"#;
			}
			_ => todo!(""),
		}
		result
	}

	fn sort_entry(
		entrymap: &HashMap<String, Option<String>>,
		left_title: &str,
		right_title: &str,
	) -> std::cmp::Ordering {
		let lp = entrymap.get(left_title).unwrap();
		let rp = entrymap.get(right_title).unwrap();

		if lp.clone().map(|s| s.as_str() == right_title) == Some(true) {
			std::cmp::Ordering::Greater
		} else if rp.clone().map(|s| s.as_str() == left_title) == Some(true) {
			std::cmp::Ordering::Less
		} else if lp.is_some() && rp.is_none() {
			std::cmp::Ordering::Greater
		} else if rp.is_some() && lp.is_none() {
			std::cmp::Ordering::Less
		} else if let (Some(pl), Some(pr)) = (lp, rp) {
			if pl == pr {
				left_title.cmp(right_title)
			} else {
				Self::sort_entry(entrymap, pl, pr)
			}
		} else {
			left_title.cmp(right_title)
		}
	}
}

pub fn create_navigation(
	docs: &Vec<(RefCell<CompiledDocument>, Option<PostProcess>)>,
) -> Result<NavEntries, String> {
	let mut nav = NavEntries {
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
		let previous = doc_borrow.get_variable("nav.previous").cloned();
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
					nav.children.insert(cat.clone(), NavEntries::default());
					nav.children.get_mut(cat.as_str()).unwrap()
				}
			};

			match cat_ent.children.get_mut(subcat.as_str()) {
				Some(subcat_ent) => subcat_ent,
				None => {
					// Insert
					cat_ent
						.children
						.insert(subcat.clone(), NavEntries::default());
					cat_ent.children.get_mut(subcat.as_str()).unwrap()
				}
			}
		} else if let Some(cat) = cat {
			match nav.children.get_mut(cat.as_str()) {
				Some(cat_ent) => cat_ent,
				None => {
					// Insert
					nav.children.insert(cat.clone(), NavEntries::default());
					nav.children.get_mut(cat.as_str()).unwrap()
				}
			}
		} else {
			&mut nav
		};

		// Find duplicates titles in current parent
		for entry in &pent.entries {
			if &entry.title == title {
				return Err(format!(
					"Conflicting entry title `{title}` for entries with the same parent: ({})",
					pent.entries
						.iter()
						.map(|entry| entry.title.clone())
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

		pent.entries.push(NavEntry {
			title: title.clone(),
			path: path.clone(),
			previous,
		});
	}

	// Sort entries
	fn sort_entries(nav: &mut NavEntries) {
		let entrymap = nav
			.entries
			.iter()
			.map(|ent| (ent.title.clone(), ent.previous.clone()))
			.collect::<HashMap<String, Option<String>>>();
		nav.entries
			.sort_by(|l, r| NavEntries::sort_entry(&entrymap, l.title.as_str(), r.title.as_str()));

		for child in nav.children.values_mut() {
			sort_entries(child);
		}
	}
	sort_entries(&mut nav);

	Ok(nav)
}
*/

/*
#[cfg(test)]
mod tests {
	use rand::prelude::SliceRandom;
	use rand::rngs::OsRng;

	use crate::compiler::process::process_from_memory;

	use super::*;

	#[test]
	fn sort() {
		let entries: Vec<NavEntry> = vec![
			NavEntry {
				title: "Index".into(),
				path: "".into(),
				previous: None,
			},
			NavEntry {
				title: "AB".into(),
				path: "".into(),
				previous: Some("Index".into()),
			},
			NavEntry {
				title: "Getting Started".into(),
				path: "".into(),
				previous: Some("Index".into()),
			},
			NavEntry {
				title: "Sections".into(),
				path: "".into(),
				previous: Some("Getting Started".into()),
			},
			NavEntry {
				title: "Style".into(),
				path: "".into(),
				previous: Some("Getting Started".into()),
			},
		];
		let mut shuffled = entries.clone();
		for _ in 0..10 {
			let mut rng = OsRng {};
			shuffled.shuffle(&mut rng);

			let entrymap = shuffled
				.iter()
				.map(|ent| (ent.title.clone(), ent.previous.clone()))
				.collect::<HashMap<String, Option<String>>>();

			shuffled.sort_by(|l, r| {
				NavEntries::sort_entry(&entrymap, l.title.as_str(), r.title.as_str())
			});

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
				NavEntry {
					title: "A".to_string(),
					path: "1.html".to_string(),
					previous: None
				},
				NavEntry {
					title: "B".to_string(),
					path: "2.html".to_string(),
					previous: None
				},
				NavEntry {
					title: "C".to_string(),
					path: "0.html".to_string(),
					previous: None
				},
			]
		);
	}
}
*/
