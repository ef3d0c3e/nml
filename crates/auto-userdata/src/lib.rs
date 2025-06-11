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

#[proc_macro_derive(AutoUserData, attributes(lua_ignore))]
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
		let name = field.ident.unwrap();
		let field_name_str = name.to_string();

		// Check for #[lua_ignore]
		let skip = field
			.attrs
			.iter()
			.any(|attr| attr.path.is_ident("lua_ignore"));
		if skip {
			continue;
		}

		field_getters.push(quote! {
			fields.add_field_method_get(#field_name_str, |_, this| {
				Ok(this.#name.clone())
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
