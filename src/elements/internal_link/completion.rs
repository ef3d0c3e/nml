use std::sync::Arc;

use ariadne::Span;
use tower_lsp::lsp_types::CompletionContext;
use tower_lsp::lsp_types::CompletionItem;
use tower_lsp::lsp_types::CompletionItemKind;
use tower_lsp::lsp_types::InsertTextFormat;
use tower_lsp::lsp_types::MarkupContent;
use tower_lsp::lsp_types::MarkupKind::Markdown;

use crate::lsp::completion::context_triggered;
use crate::lsp::completion::CompletionProvider;
use crate::lsp::data::LangServerData;
use crate::lsp::reference::LsReference;
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
			reference.source_path.display(),
			reference.range.start(),
			reference.range.end(),
			reference.reftype
		)
	}

	pub(crate) fn export_internal_ref(
		unit: &TranslationUnit,
		lsp: &mut LangServerData,
		elem: Arc<dyn ReferenceableElement>,
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
				source_path: iref.location().source().name().clone(),
				source_refkey: unit.reference_key().to_owned(),
				reftype: referenceable.refcount_key().to_owned(),
			},
		);
	}
}

impl CompletionProvider for ReferenceCompletion {
	fn trigger(&self) -> &'static [&'static str] {
		["&"].as_slice()
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

	fn static_items(&self, context: &Option<CompletionContext>, items: &mut Vec<CompletionItem>) {
		// &{ref}
		items.push(CompletionItem {
			label: "&{ref}".to_string(),
			detail: Some("Link to reference".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
				MarkupContent {
					kind: tower_lsp::lsp_types::MarkupKind::Markdown,
					value: "# Usage

`&{REF}` Link to reference `REF`
`&{REF}[DISP]` Link to reference `REF` displayed using `DISP`

Create a link to a reference.

# Examples

 * `&{foo}` *Will display a link to reference **foo***
 * `&{bar}[click me]` *Will display `click me` that will link to reference **bar***
 * `&{source#baz}` *Will display a link to reference **baz** declared in unit **source***"
						.into(),
				},
			)),
			kind: Some(CompletionItemKind::SNIPPET),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
				"{}{{${{1:REFNAME}}}}",
				if context_triggered(context, "&") {
					""
				} else {
					"&"
				}
			)),
			..CompletionItem::default()
		});

		// &{ref}[disp]
		items.push(CompletionItem {
			label: "&{ref}[disp]".to_string(),
			detail: Some("Link to reference with display".into()),
			documentation: Some(tower_lsp::lsp_types::Documentation::MarkupContent(
				MarkupContent {
					kind: tower_lsp::lsp_types::MarkupKind::Markdown,
					value: "# Usage

`&{REF}` Link to reference `REF`
`&{REF}[DISP]` Link to reference `REF` displayed using `DISP`

Create a link to a reference.

# Examples

 * `&{foo}` *Will display a link to reference **foo***
 * `&{bar}[click me]` *Will display `click me` that will link to reference **bar***
 * `&{source#baz}` *Will display a link to reference **baz** declared in unit **source***"
						.into(),
				},
			)),
			kind: Some(CompletionItemKind::SNIPPET),
			insert_text_format: Some(InsertTextFormat::SNIPPET),
			insert_text: Some(format!(
				"{}{{${{1:REFNAME}}}}[${{2:DISPLAY}}]",
				if context_triggered(context, "&") {
					""
				} else {
					"&"
				}
			)),
			..CompletionItem::default()
		});
	}
}
