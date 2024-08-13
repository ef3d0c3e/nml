use std::cell::RefCell;

use crate::document::document::CrossReference;

use super::compiler::CompiledDocument;
use super::compiler::Target;

/// Represents the list of tasks that have to be ran after the document has been compiled and the
/// compiled document list has been built. Every task is stored with a raw byte position in the
/// compiled document's body. The position represents the original position and thus should be
/// offset accordingly to other postprocessing tasks.
pub struct PostProcess {
	/// List of references to resolve i.e insert the resolved refname at a certain byte position
	/// in the document's body
	pub resolve_references: Vec<(usize, CrossReference)>,
}

impl PostProcess {
	/// Applies postprocessing to a [`CompiledDocument`]
	pub fn apply(
		&self,
		target: Target,
		list: &Vec<(RefCell<CompiledDocument>, Option<PostProcess>)>,
		doc: &RefCell<CompiledDocument>,
	) -> Result<String, String> {
		let mut content = doc.borrow().body.clone();

		let mut offset = 0;
		for (pos, cross_ref) in &self.resolve_references {
			// Cross-references
			let mut found_ref: Option<(String, &RefCell<CompiledDocument>)> = None;
			match cross_ref {
				CrossReference::Unspecific(name) => {
					for (doc, _) in list {
						if let Some(found) = doc.borrow().references.get(name) {
							// Check for duplicates
							if let Some((_, previous_doc)) = &found_ref {
								return Err(format!("Cannot use an unspecific reference for reference named: `{found}`. Found in document `{}` but also in `{}`. Specify the source of the reference to resolve the conflict.", previous_doc.borrow().input, doc.borrow().input));
							}

							found_ref = Some((found.clone(), &doc));
						}
					}
				}
				CrossReference::Specific(doc_name, name) => {
					let ref_doc = list.iter().find(|(doc, _)| {
						let doc_borrow = doc.borrow();
						if let Some(outname) = doc_borrow.variables.get("compiler.output") {
							// Strip extension
							let split_at = outname.rfind('.').unwrap_or(outname.len());
							return doc_name == outname.split_at(split_at).0;
						}

						false
					});
					if ref_doc.is_none() {
						return Err(format!(
							"Cannot find document `{doc_name}` for reference `{name}` in `{}`",
							doc.borrow().input
						));
					}

					if let Some(found) = ref_doc.unwrap().0.borrow().references.get(name) {
						found_ref = Some((found.clone(), &ref_doc.unwrap().0));
					}
				}
			}
			if let Some((found_ref, found_doc)) = &found_ref {
				let found_borrow = found_doc.borrow();
				let found_path = found_borrow.get_variable("compiler.output").ok_or(format!(
					"Unable to get the output. Aborting postprocessing."
				))?;
				let insert_content = format!("{found_path}#{found_ref}");
				content.insert_str(pos - offset, insert_content.as_str());
				offset += insert_content.len();
			} else {
				return Err(format!("Cannot find reference `{cross_ref}` from document `{}`. Aborting postprocessing.", doc.borrow().input));
			}
		}

		Ok(content)
	}
}
