//! `#[derive(Diagnostic)]` for the central diagnostic enum.
//!
//! The generated code refers to `::nml_diagnostics`, so the derive is meant for the
//! `DiagnosticKind` enum of the `nml-diagnostics` crate.

use std::collections::HashMap;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::format_ident;
use quote::quote;
use syn::Data;
use syn::DeriveInput;
use syn::Fields;
use syn::Ident;
use syn::LitInt;
use syn::LitStr;
use syn::parse_macro_input;

/// Derives the metadata of a diagnostic kind enum.
///
/// Every variant needs `#[diag(id = <u32>, message = "<fluent id>")]`, and may set
/// `severity = error | warning | hint` (default: `error`). Variants have named fields or none;
/// each field becomes a Fluent argument of the same name.
///
/// Generates, as inherent methods: `id`, `message_id`, `default_severity`, `args`, and the
/// constant `KINDS` listing every variant. A duplicate `id` is a compile error.
#[proc_macro_derive(Diagnostic, attributes(diag))]
pub fn derive_diagnostic(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as DeriveInput);
	match expand(input) {
		Ok(tokens) => tokens.into(),
		Err(error) => error.to_compile_error().into(),
	}
}

struct Variant {
	ident: Ident,
	id: u32,
	message: String,
	severity: Ident,
	fields: Vec<Ident>,
}

fn expand(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
	let Data::Enum(data) = &input.data else {
		return Err(syn::Error::new_spanned(
			&input.ident,
			"Diagnostic can only be derived for enums",
		));
	};

	let mut variants = Vec::new();
	let mut ids: HashMap<u32, Ident> = HashMap::new();
	for variant in &data.variants {
		let parsed = parse_variant(variant)?;
		if let Some(previous) = ids.insert(parsed.id, parsed.ident.clone()) {
			return Err(syn::Error::new_spanned(
				&variant.ident,
				format!(
					"diagnostic id {} is already used by `{previous}`",
					parsed.id
				),
			));
		}
		variants.push(parsed);
	}

	let name = &input.ident;
	let patterns: Vec<_> = variants
		.iter()
		.map(|variant| {
			let ident = &variant.ident;
			let fields = &variant.fields;
			quote! { Self::#ident { #(#fields),* } }
		})
		.collect();
	let ids = variants.iter().map(|variant| variant.id);
	let messages = variants.iter().map(|variant| &variant.message);
	let severities: Vec<_> = variants
		.iter()
		.map(|variant| {
			let severity = &variant.severity;
			quote! { ::nml_diagnostics::DiagnosticSeverity::#severity }
		})
		.collect();
	let args = variants.iter().map(|variant| {
		let names = variant.fields.iter().map(|field| field.to_string());
		let fields = &variant.fields;
		quote! {
			vec![#((#names, ::nml_diagnostics::Argument::from(::std::clone::Clone::clone(#fields)))),*]
		}
	});
	let kinds = variants.iter().zip(&severities).map(|(variant, severity)| {
		let id = variant.id;
		let ident = variant.ident.to_string();
		let message = &variant.message;
		let fields = variant.fields.iter().map(|field| field.to_string());
		quote! {
			::nml_diagnostics::KindInfo {
				id: #id,
				name: #ident,
				message_id: #message,
				severity: #severity,
				fields: &[#(#fields),*],
			}
		}
	});

	let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
	Ok(quote! {
		impl #impl_generics #name #type_generics #where_clause {
			/// Every kind, in declaration order.
			pub const KINDS: &'static [::nml_diagnostics::KindInfo] = &[#(#kinds),*];

			/// The stable numeric ID of this kind.
			pub fn id(&self) -> u32 {
				#[allow(unused_variables)]
				match self { #(#patterns => #ids),* }
			}

			/// The Fluent message ID of this kind.
			pub fn message_id(&self) -> &'static str {
				#[allow(unused_variables)]
				match self { #(#patterns => #messages),* }
			}

			/// The severity of this kind unless overridden.
			pub fn default_severity(&self) -> ::nml_diagnostics::DiagnosticSeverity {
				#[allow(unused_variables)]
				match self { #(#patterns => #severities),* }
			}

			/// The Fluent arguments of this kind: one per field, named after the field.
			pub fn args(&self) -> ::std::vec::Vec<(&'static str, ::nml_diagnostics::Argument)> {
				match self { #(#patterns => #args),* }
			}
		}
	})
}

fn parse_variant(variant: &syn::Variant) -> syn::Result<Variant> {
	let fields = match &variant.fields {
		Fields::Named(named) => named
			.named
			.iter()
			.map(|field| field.ident.clone().expect("named field"))
			.collect(),
		Fields::Unit => Vec::new(),
		Fields::Unnamed(_) => {
			return Err(syn::Error::new_spanned(
				variant,
				"diagnostic kinds need named fields: they become Fluent arguments",
			));
		}
	};

	let mut id = None;
	let mut message = None;
	let mut severity = None;
	let mut found = false;
	for attribute in variant.attrs.iter().filter(|a| a.path().is_ident("diag")) {
		found = true;
		attribute.parse_nested_meta(|meta| {
			if meta.path.is_ident("id") {
				id = Some(meta.value()?.parse::<LitInt>()?.base10_parse::<u32>()?);
			} else if meta.path.is_ident("message") {
				let value = meta.value()?.parse::<LitStr>()?;
				if !is_fluent_id(&value.value()) {
					return Err(syn::Error::new_spanned(
						&value,
						"not a valid Fluent message ID",
					));
				}
				message = Some(value.value());
			} else if meta.path.is_ident("severity") {
				let value = meta.value()?.parse::<Ident>()?;
				severity = Some(match value.to_string().as_str() {
					"error" => format_ident!("Error"),
					"warning" => format_ident!("Warning"),
					"hint" => format_ident!("Hint"),
					_ => {
						return Err(syn::Error::new_spanned(
							&value,
							"expected `error`, `warning` or `hint`",
						));
					}
				});
			} else {
				return Err(meta.error("expected `id`, `message` or `severity`"));
			}
			Ok(())
		})?;
	}
	if !found {
		return Err(syn::Error::new_spanned(
			&variant.ident,
			"missing #[diag(id = ..., message = \"...\")]",
		));
	}
	let missing = |what| syn::Error::new_spanned(&variant.ident, format!("missing `{what}`"));
	Ok(Variant {
		ident: variant.ident.clone(),
		id: id.ok_or_else(|| missing("id"))?,
		message: message.ok_or_else(|| missing("message"))?,
		severity: severity.unwrap_or_else(|| Ident::new("Error", Span::call_site())),
		fields,
	})
}

/// Fluent identifiers: `[a-zA-Z][a-zA-Z0-9_-]*`.
fn is_fluent_id(id: &str) -> bool {
	let mut chars = id.chars();
	chars.next().is_some_and(|c| c.is_ascii_alphabetic())
		&& chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
