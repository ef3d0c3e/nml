#![feature(proc_macro_span)]
use std::cell::RefCell;
use std::collections::HashMap;

use lazy_static::lazy_static;
use proc_macro::TokenStream;
use quote::quote;
use std::sync::Mutex;
use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::parse_macro_input;
use syn::DeriveInput;
use syn::Fields;
use syn::ItemStruct;

#[proc_macro_derive(AutoUserData, attributes(lua_ignore, lua_map))]
pub fn derive_lua_user_data(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as DeriveInput);
	let ident = input.ident;

	let fields = match input.data {
		syn::Data::Struct(data) => match data.fields {
			Fields::Named(named) => named.named,
			_ => {
				return syn::Error::new_spanned(
					ident,
					"AutoUserData only supports named struct fields",
				)
				.to_compile_error()
				.into();
			}
		},
		_ => {
			return syn::Error::new_spanned(ident, "Only structs supported")
				.to_compile_error()
				.into();
		}
	};

	let mut field_getters = Vec::new();

	for field in fields {
		let name = field.ident.clone().unwrap();
		let field_name_str = name.to_string();

		let mut skip = false;
		let mut map_wrapper = None;

		for attr in &field.attrs {
			if attr.path.is_ident("lua_ignore") {
				skip = true;
				break;
			}

			if attr.path.is_ident("lua_map") {
				let meta = attr.parse_meta();
				if let Ok(syn::Meta::List(meta_list)) = meta {
					if let Some(syn::NestedMeta::Meta(syn::Meta::Path(path))) =
						meta_list.nested.first()
					{
						if let Some(ident) = path.get_ident() {
							map_wrapper = Some(ident.clone());
						}
					}
				}
			}
		}

		if skip {
			continue;
		}

		let getter_expr = if let Some(wrapper_ident) = map_wrapper {
			quote! { Ok(#wrapper_ident { inner: this.#name.clone() }) }
		} else {
			quote! { Ok(this.#name.clone()) }
		};

		field_getters.push(quote! {
			fields.add_field_method_get(#field_name_str, |_, this| {
				#getter_expr
			});
		});
	}

	let expanded = quote! {
		impl mlua::UserData for #ident {
			fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(fields: &mut F) {
				#(#field_getters)*
			}
		}
	};

	TokenStream::from(expanded)
}
