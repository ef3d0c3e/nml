#![feature(proc_macro_span)]
use proc_macro::TokenStream;
use quote::quote;
use quote::ToTokens;
use quote::TokenStreamExt;
use syn::parse_macro_input;
use syn::DeriveInput;
use syn::Fields;
use syn::Ident;
use syn::Lit;
use syn::Meta;
use syn::NestedMeta;

enum ValueMapper {
	Ignore,
	Value,
	ArcDeref,
	Map(Ident, Option<String>),
}

#[proc_macro_derive(
	AutoUserData,
	attributes(lua_value, lua_ignore, lua_map, lua_arc_deref, auto_userdata_target)
)]
pub fn derive_lua_user_data(input: TokenStream) -> TokenStream {
	let input = parse_macro_input!(input as DeriveInput);
	let ident = input.ident.clone();

	let mut targets : Vec<proc_macro2::TokenStream> = vec![];

	for attr in &input.attrs {
		if attr.path.is_ident("auto_userdata_target") {
			if let Ok(Meta::NameValue(nv)) = attr.parse_meta() {
				if let Lit::Str(litstr) = nv.lit {
					let value = litstr.value();
					if value == "&" {
						targets.push(quote! { & #ident });
					} else if value == "&mut" {
						targets.push(quote! { &mut #ident });
					} else if value == "*" {
						targets.push(quote! { #ident });
					} else {
						return syn::Error::new_spanned(
							litstr,
							"Only `&` is supported as target currently",
						)
						.to_compile_error()
						.into();
					}
				}
			}
		}
	}
	if targets.is_empty()
	{
		targets.push(quote! { #ident });
	}

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

		let mut mapper: Option<ValueMapper> = None;

		for attr in &field.attrs {
			if attr.path.is_ident("lua_ignore") {
				mapper = Some(ValueMapper::Ignore);
				break;
			}
			if attr.path.is_ident("lua_value") {
				mapper = Some(ValueMapper::Value);
				break;
			}
			if attr.path.is_ident("lua_arc_deref") {
				mapper = Some(ValueMapper::ArcDeref);
				break;
			}
			if attr.path.is_ident("lua_map") {
				if let Ok(Meta::List(meta_list)) = attr.parse_meta() {
					let mut map_to = None;
					let mut map_expr = None;

					if let Some(NestedMeta::Meta(Meta::Path(path))) = meta_list.nested.iter().nth(0)
					{
						if let Some(ident) = path.get_ident() {
							map_to = Some(ident.clone());
						}
					}

					if let Some(NestedMeta::Meta(Meta::Path(path))) = meta_list.nested.iter().nth(1)
					{
						map_expr = Some(path.to_token_stream().to_string());
					}

					if let Some(map_to) = map_to {
						mapper = Some(ValueMapper::Map(map_to, map_expr));
					}
				}
				break;
			}
		}

		let getter_expr = match mapper {
			Some(ValueMapper::Ignore) => continue,
			Some(ValueMapper::Map(mapper, expr)) => {
				if let Some(expr) = expr {
					let code = expr.replace("$", "#name");
					quote! {
						Ok(#mapper(this.#code))
					}
				} else {
					quote! {
						Ok(#mapper(this.#name.clone()))
					}
				}
			}
			Some(ValueMapper::ArcDeref) => quote! {
				let r: &'static _ = unsafe { &*Arc::as_ptr(&this.#name) };
				Ok(lua.create_userdata(r).unwrap())
			},
			Some(ValueMapper::Value) => quote! { lua.to_value(&this.#name) },
			_ => quote! { Ok(this.#name.clone()) },
		};

		field_getters.push(quote! {
			fields.add_field_method_get(#field_name_str, |lua, this| {
				#getter_expr
			});
		});
	}

	let mut expanded : proc_macro2::TokenStream = Default::default();
	for target in targets {
		expanded.extend(quote!{
			impl mlua::UserData for #target {
				fn add_fields<F: mlua::UserDataFields<Self>>(fields: &mut F) {
					#(#field_getters)*
				}
			}
		});
	}

	TokenStream::from(expanded)
}
