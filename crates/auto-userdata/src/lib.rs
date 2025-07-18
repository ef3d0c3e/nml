#![feature(proc_macro_span)]
use proc_macro::TokenStream;
use quote::quote;
use syn::Ident;
use syn::parse_macro_input;
use syn::DeriveInput;
use syn::Fields;

enum ValueMapper
{
	Ignore,
	Value,
	Map(Ident),
}

#[proc_macro_derive(AutoUserData, attributes(lua_value, lua_ignore, lua_map))]
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
		let mut mapper = None;

		for attr in &field.attrs {
			if attr.path.is_ident("lua_ignore") {
				mapper = Some(ValueMapper::Ignore);
				break;
			}
			if attr.path.is_ident("lua_value") {
				mapper = Some(ValueMapper::Value);
				break;
			}
			if attr.path.is_ident("lua_map") {
				let meta = attr.parse_meta();
				if let Ok(syn::Meta::List(meta_list)) = meta {
					if let Some(syn::NestedMeta::Meta(syn::Meta::Path(path))) =
						meta_list.nested.first()
					{
						if let Some(ident) = path.get_ident() {
							mapper = Some(ValueMapper::Map(ident.clone()))
						}
					}
				}
				break;
			}
		}

		let getter_expr = match mapper
		{
			Some(ValueMapper::Ignore) => continue,
			Some(ValueMapper::Map(mapper)) => quote! { Ok(#mapper { inner: this.#name.clone() }) },
			Some(ValueMapper::Value) => quote! { lua.to_value(&this.#name) },
			_ => quote! { Ok(this.#name.clone()) },
		};

		field_getters.push(quote! {
			fields.add_field_method_get(#field_name_str, |lua, this| {
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
