use std::fmt::format;
use std::rc::Rc;

use ariadne::Span;
use tower_lsp::lsp_types::CompletionContext;
use tower_lsp::lsp_types::CompletionItem;
use tower_lsp::lsp_types::CompletionItemKind;
use tower_lsp::lsp_types::MarkupContent;
use tower_lsp::lsp_types::MarkupKind::Markdown;

use crate::lsp::completion::CompletionProvider;
use crate::lsp::data::LangServerData;
use crate::lsp::reference::LsReference;
use crate::unit::element::Element;
use crate::unit::element::ReferenceableElement;
use crate::unit::scope::ScopeAccessor;
use crate::unit::translation::TranslationUnit;

pub struct ReferenceCompletion;

impl ReferenceCompletion {
	pub(crate) fn get_documentation(reference: &LsReference, label: &String) -> String {
		format!(
			"Reference `{}`

Full name: [{label}]()

# Definition

 * **Defined in**: [{}]() ({}..{})
 * **Type**: `{}`",
			reference.name,
			reference.source_path,
			reference.range.start(),
			reference.range.end(),
			reference.reftype
		)
	}

	pub(crate) fn export_internal_ref(
		unit: &TranslationUnit,
		lsp: &mut LangServerData,
		elem: Rc<dyn ReferenceableElement>,
	) {
		let Some(referenceable) = elem.as_referenceable() else {
			return;
		};
		let iref = referenceable.reference();
		let label = iref.name().to_string();
		lsp.external_refs.insert(
			label.clone(),
			LsReference {
				name: label,
				range: iref.location().range.clone(),
				source_path: iref.location().source().name().to_owned(),
				source_refkey: unit.reference_key().to_owned(),
				reftype: referenceable.refcount_key().to_owned(),
			},
		);
	}
}

impl CompletionProvider for ReferenceCompletion {
	fn trigger(&self) -> &'static [&'static str] {
		[].as_slice()
	}

	fn unit_items(&self, unit: &TranslationUnit, items: &mut Vec<CompletionItem>) {
		let start = format!("{}#", unit.reference_key());
		unit.with_lsp(|mut lsp| {
			unit.get_entry_scope()
				.content_iter(true)
				.filter_map(|(_, elem)| elem.as_referenceable())
				.for_each(|referenceable| {
					Self::export_internal_ref(unit, &mut lsp, referenceable);
				});

			lsp.external_refs.iter().for_each(|(label, reference)| {
				if label.starts_with(&start) {
					return;
				}
				items.push(CompletionItem {
					label: label.clone(),
					detail: Some(format!("Reference {label}")),
					documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
						MarkupContent {
							kind: Markdown,
							value: Self::get_documentation(reference, label),
						},
					)),
					kind: Some(CompletionItemKind::REFERENCE),
					..CompletionItem::default()
				});
			});
		});
	}

	fn static_items(&self, _context: &Option<CompletionContext>, _items: &mut Vec<CompletionItem>) {
	}
}
