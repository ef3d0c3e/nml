use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
	parse::Parse, parse::ParseStream, parse_macro_input, punctuated::Punctuated, DeriveInput,
	Field, Ident, Meta, Token,
};

// Helper attributes
#[proc_macro_attribute]
pub fn lua_proxy(_args: TokenStream, input: TokenStream) -> TokenStream {
	input
}

#[proc_macro_attribute]
pub fn lua_ud(_args: TokenStream, input: TokenStream) -> TokenStream {
	input
}

#[proc_macro_attribute]
pub fn lua_value(_args: TokenStream, input: TokenStream) -> TokenStream {
	input
}

#[proc_macro_attribute]
pub fn lua_vec(_args: TokenStream, input: TokenStream) -> TokenStream {
	input
}

#[proc_macro_attribute]
pub fn lua_ignore(_args: TokenStream, input: TokenStream) -> TokenStream {
	input
}

// Parsing
struct AutoUserDataArgs {
	proxy: Option<String>,
	immutable: bool,
	mutable: bool,
}

impl Parse for AutoUserDataArgs {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		let mut proxy = None;
		let mut immutable = false;
		let mut mutable = false;

		let args = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;
		for meta in args {
			match &meta {
				Meta::NameValue(nv) if nv.path.is_ident("proxy") => {
					if let syn::Expr::Lit(syn::ExprLit {
						lit: syn::Lit::Str(s),
						..
					}) = &nv.value
					{
						proxy = Some(s.value());
					} else {
						return Err(syn::Error::new_spanned(
							&nv.value,
							"proxy must be a string literal",
						));
					}
				}
				Meta::Path(p) if p.is_ident("immutable") => immutable = true,
				Meta::Path(p) if p.is_ident("mutable") => mutable = true,
				_ => return Err(syn::Error::new_spanned(&meta, "unknown argument")),
			}
		}

		Ok(AutoUserDataArgs {
			proxy,
			immutable,
			mutable,
		})
	}
}

// Fields

/// The smart pointer wrapper around the field, if any.
enum ProxyWrapper {
	/// Plain field
	None,
	/// Arc<T> as_ptr() for const, panic for mut
	Arc,
	/// Box<T> deref
	Box,
	/// Rc<T> as_ptr() for const, panic for mut
	Rc,
}

impl ProxyWrapper {
	fn parse(ident: &Ident) -> syn::Result<Self> {
		match ident.to_string().as_str() {
			"Arc" => Ok(ProxyWrapper::Arc),
			"Box" => Ok(ProxyWrapper::Box),
			"Rc" => Ok(ProxyWrapper::Rc),
			other => Err(syn::Error::new_spanned(
				ident,
				format!("unsupported wrapper `{other}`, expected Arc, Box, or Rc"),
			)),
		}
	}

	/// Whether this wrapper can yield a mutable pointer at all.
	fn supports_mut(&self) -> bool {
		matches!(self, ProxyWrapper::None | ProxyWrapper::Box)
	}
}

enum FieldKind {
	/// #[lua_proxy(ProxyName)] or #[lua_proxy(ProxyName, Arc)]
	/// or #[lua_proxy(ProxyName, Arc, immutable)] etc.
	Proxy {
		base: Ident,
		wrapper: ProxyWrapper,
		force_immutable: bool,
	},
	/// #[lua_vec(Wrapper, ElemType)] -- get only
	/// #[lua_vec(Wrapper, OwnedType, ElemType)] -- get + set (mut proxy only)
	/// get: Wrapper::<ElemType>(&field as *const _).into_lua(lua)
	/// set: OwnedType::<ElemType>::from_lua(val, lua)?.0
	LuaVec {
		base: Ident,
		owned: Option<Ident>,
		elem: Ident,
	},
	/// #[lua_ud] clone field value, use its own UserData impl (requires Clone)
	UserDataSelf,
	/// #[lua_ud(SomeType)] wrap *mut field in SomeType, which manages its own UserData
	UserDataWrapper(Ident),
	/// #[lua_udc(SomeType)] wrap *mut field in SomeType, which manages its own UserData
	UserDataCopyWrapper(Ident),
	/// #[lua_value] serde via mlua's serialize feature
	Value,
	/// #[lua_ignore] not exposed to Lua
	Ignore,
	/// No attribute default IntoLua/FromLua
	Default,
}

/// Parse #[lua_proxy(ProxyName)]
///      #[lua_proxy(ProxyName, immutable)]
///      #[lua_proxy(ProxyName, Arc)]
///      #[lua_proxy(ProxyName, Arc, immutable)]
fn parse_lua_proxy(attr: &syn::Attribute) -> syn::Result<FieldKind> {
	let mut base: Option<Ident> = None;
	let mut wrapper = ProxyWrapper::None;
	let mut force_immutable = false;

	attr.parse_args_with(|input: ParseStream| {
		// First arg: proxy base name
		base = Some(input.parse::<Ident>()?);

		// Optional second arg: wrapper or immutable
		if input.peek(Token![,]) {
			let _: Token![,] = input.parse()?;
			let ident: Ident = input.parse()?;
			if ident == "immutable" {
				force_immutable = true;
			} else {
				wrapper = ProxyWrapper::parse(&ident)?;

				// Optional third arg: immutable
				if input.peek(Token![,]) {
					let _: Token![,] = input.parse()?;
					let flag: Ident = input.parse()?;
					if flag == "immutable" {
						force_immutable = true;
					} else {
						return Err(syn::Error::new_spanned(flag, "expected `immutable`"));
					}
				}
			}
		}

		Ok(())
	})?;

	// Arc and Rc cannot yield *mut force_immutable is mandatory.
	if matches!(wrapper, ProxyWrapper::Arc | ProxyWrapper::Rc) && !force_immutable {
		return Err(syn::Error::new(
			attr.pound_token.spans[0],
			"Arc and Rc fields cannot yield *mut, add `immutable`: \
			 #[lua_proxy(ProxyName, Arc, immutable)]",
		));
	}

	Ok(FieldKind::Proxy {
		base: base.unwrap(),
		wrapper,
		force_immutable,
	})
}

/// Parse #[lua_vec(Wrapper, ElemType)]
///      #[lua_vec(Wrapper, OwnedType, ElemType)]
fn parse_lua_vec(attr: &syn::Attribute) -> syn::Result<FieldKind> {
	let mut base: Option<Ident> = None;
	let mut second: Option<Ident> = None;
	let mut third: Option<Ident> = None;

	attr.parse_args_with(|input: ParseStream| {
		base = Some(input.parse::<Ident>()?);

		if input.peek(Token![,]) {
			let _: Token![,] = input.parse()?;
			second = Some(input.parse::<Ident>()?);
		}

		if input.peek(Token![,]) {
			let _: Token![,] = input.parse()?;
			third = Some(input.parse::<Ident>()?);
		}

		Ok(())
	})?;

	// Two idents:   (Wrapper, ElemType) get only
	// Three idents: (Wrapper, OwnedType, ElemType) get + set
	let (owned, elem) = match (second, third) {
		(Some(s), Some(t)) => (Some(s), t),
		(Some(s), None) => (None, s),
		_ => {
			return Err(syn::Error::new(
				attr.pound_token.spans[0],
				"lua_vec requires at least two arguments: #[lua_vec(Wrapper, ElemType)]",
			))
		}
	};

	Ok(FieldKind::LuaVec {
		base: base.unwrap(),
		owned,
		elem,
	})
}

fn classify_field(field: &Field) -> syn::Result<FieldKind> {
	for attr in &field.attrs {
		if attr.path().is_ident("lua_proxy") {
			return parse_lua_proxy(attr);
		}
		if attr.path().is_ident("lua_vec") {
			return parse_lua_vec(attr);
		}
		if attr.path().is_ident("lua_ud") {
			// No args: use the field type's own UserData impl (must be Clone)
			if matches!(attr.meta, syn::Meta::Path(_)) {
				return Ok(FieldKind::UserDataSelf);
			}
			let ud_type: Ident = attr.parse_args()?;
			return Ok(FieldKind::UserDataWrapper(ud_type));
		}
		if attr.path().is_ident("lua_udc") {
			let ud_type: Ident = attr.parse_args()?;
			return Ok(FieldKind::UserDataCopyWrapper(ud_type));
		}
		if attr.path().is_ident("lua_value") {
			return Ok(FieldKind::Value);
		}
		if attr.path().is_ident("lua_ignore") {
			return Ok(FieldKind::Ignore);
		}
	}
	Ok(FieldKind::Default)
}

// Generation
fn generate_proxy(
	proxy_name: &Ident,
	is_mut: bool,
	struct_name: &Ident,
	fields: &Punctuated<Field, Token![,]>,
) -> syn::Result<TokenStream2> {
	let ptr_type = if is_mut {
		quote! { *mut #struct_name }
	} else {
		quote! { *const #struct_name }
	};

	let mut field_tokens = TokenStream2::new();

	for field in fields {
		let field_name = field.ident.as_ref().unwrap();
		let field_name_str = field_name.to_string();
		let field_ty = &field.ty;
		let kind = classify_field(field)?;

		match kind {
			FieldKind::Ignore => {}

			FieldKind::Default => {
				field_tokens.extend(quote! {
					fields.add_field_method_get(#field_name_str, |_, this| {
						Ok(unsafe { (*this.0).#field_name.clone() })
					});
				});
				if is_mut {
					field_tokens.extend(quote! {
						fields.add_field_method_set(#field_name_str, |_, this, val| {
							unsafe { (*this.0).#field_name = val };
							Ok(())
						});
					});
				}
			}

			FieldKind::Value => {
				field_tokens.extend(quote! {
					fields.add_field_method_get(#field_name_str, |lua, this| {
						mlua::LuaSerdeExt::to_value(lua, unsafe { &(*this.0).#field_name })
					});
				});
				if is_mut {
					field_tokens.extend(quote! {
						fields.add_field_method_set(#field_name_str, |lua, this, val: ::mlua::Value| {
							unsafe { (*this.0).#field_name = mlua::LuaSerdeExt::from_value(lua, val)?; }
							Ok(())
						});
					});
				}
			}

			// #[lua_vec(Wrapper, ElemType)] get only
			// #[lua_vec(Wrapper, OwnedType, ElemType)] get + set (mut proxy only)
			// get: Wrapper::<ElemType>(&field as *const _).into_lua(lua)
			// set: OwnedType::<ElemType>::from_lua(val, lua)?.0
			FieldKind::LuaVec {
				ref base,
				ref owned,
				ref elem,
			} => {
				field_tokens.extend(quote! {
					fields.add_field_method_get(#field_name_str, |lua, this| {
						::mlua::IntoLua::into_lua(
							#base::<#elem>(unsafe { &(*this.0).#field_name as *const _ }),
							lua,
						)
					});
				});

				if is_mut {
					if let Some(ref owned_type) = owned {
						field_tokens.extend(quote! {
							fields.add_field_method_set(#field_name_str, |lua, this, val: ::mlua::Value| {
								unsafe {
									(*this.0).#field_name =
										<#owned_type::<#elem> as ::mlua::FromLua>::from_lua(val, lua)?.0;
								}
								Ok(())
							});
						});
					}
				}
			}

			FieldKind::Proxy {
				ref base,
				ref wrapper,
				force_immutable,
			} => {
				let use_mut = is_mut && !force_immutable && wrapper.supports_mut();

				let proxy_ident = if use_mut {
					Ident::new(&format!("{base}Mut"), base.span())
				} else {
					base.clone()
				};

				let ptr_expr = match wrapper {
					ProxyWrapper::None => {
						if use_mut {
							quote! { &mut (*this.0).#field_name as *mut _ }
						} else {
							quote! { &(*this.0).#field_name as *const _ }
						}
					}
					ProxyWrapper::Box => {
						if use_mut {
							quote! { (*this.0).#field_name.as_mut() as *mut _ }
						} else {
							quote! { (*this.0).#field_name.as_ref() as *const _ }
						}
					}
					ProxyWrapper::Arc => {
						quote! { ::std::sync::Arc::as_ptr(&(*this.0).#field_name) }
					}
					ProxyWrapper::Rc => {
						quote! { ::std::rc::Rc::as_ptr(&(*this.0).#field_name) }
					}
				};

				field_tokens.extend(quote! {
					fields.add_field_method_get(#field_name_str, |lua, this| {
						let ptr = unsafe { #ptr_expr };
						lua.create_userdata(#proxy_ident(ptr))
					});
				});
			}

			// #[lua_ud] clone the field value and use its own UserData impl
			FieldKind::UserDataSelf => {
				field_tokens.extend(quote! {
					fields.add_field_method_get(#field_name_str, |lua, this| {
						lua.create_userdata(unsafe { (*this.0).#field_name.clone() })
					});
				});
			}

			// #[lua_ud(SomeType)] wrap field in SomeType with const/mut pointer
			FieldKind::UserDataWrapper(ref ud_type) => {
				let ptr_expr = if is_mut {
					quote! { &mut (*this.0).#field_name as *mut _ }
				} else {
					quote! { &(*this.0).#field_name as *const _ }
				};
				field_tokens.extend(quote! {
					fields.add_field_method_get(#field_name_str, |lua, this| {
						let ptr = unsafe { #ptr_expr };
						lua.create_userdata(#ud_type(ptr))
					});
				});
			}

			// #[lua_udc(SomeType)] wrap field in SomeType with const/mut pointer
			FieldKind::UserDataCopyWrapper(ref ud_type) => {
				let ptr_expr = quote! { (*this.0).#field_name.clone() };
				field_tokens.extend(quote! {
					fields.add_field_method_get(#field_name_str, |lua, this| {
						let ptr = unsafe { #ptr_expr };
						lua.create_userdata(#ud_type(ptr))
					});
				});
			}
		}
	}

	Ok(quote! {
		pub struct #proxy_name(pub #ptr_type);

		unsafe impl Send for #proxy_name {}
		unsafe impl Sync for #proxy_name {}

		impl ::mlua::UserData for #proxy_name {
			fn add_fields<F: ::mlua::UserDataFields<Self>>(fields: &mut F) {
				#field_tokens
			}
		}
	})
}

// Macro
#[proc_macro_attribute]
pub fn auto_userdata(args: TokenStream, input: TokenStream) -> TokenStream {
	let args = parse_macro_input!(args as AutoUserDataArgs);
	let mut input = parse_macro_input!(input as DeriveInput);

	let struct_name = input.ident.clone();

	let proxy_base = args
		.proxy
		.as_deref()
		.unwrap_or(&format!("{struct_name}Proxy"))
		.to_string();

	let fields = match &input.data {
		syn::Data::Struct(s) => match &s.fields {
			syn::Fields::Named(f) => f.named.clone(),
			_ => {
				return syn::Error::new(
					Span::call_site(),
					"auto_userdata only supports named fields",
				)
				.to_compile_error()
				.into()
			}
		},
		_ => {
			return syn::Error::new(Span::call_site(), "auto_userdata only supports structs")
				.to_compile_error()
				.into()
		}
	};

	// Strip lua* attributes
	if let syn::Data::Struct(ref mut s) = input.data {
		if let syn::Fields::Named(ref mut f) = s.fields {
			for field in f.named.iter_mut() {
				field.attrs.retain(|attr| {
					!attr.path().is_ident("lua_proxy")
						&& !attr.path().is_ident("lua_vec")
						&& !attr.path().is_ident("lua_ud")
						&& !attr.path().is_ident("lua_udc")
						&& !attr.path().is_ident("lua_value")
						&& !attr.path().is_ident("lua_ignore")
				});
			}
		}
	}

	let mut output = TokenStream2::new();
	output.extend(quote! { #input });

	if args.immutable {
		let proxy_name = Ident::new(&proxy_base, Span::call_site());
		match generate_proxy(&proxy_name, false, &struct_name, &fields) {
			Ok(ts) => output.extend(ts),
			Err(e) => return e.to_compile_error().into(),
		}
	}

	if args.mutable {
		let proxy_mut_name = Ident::new(&format!("{proxy_base}Mut"), Span::call_site());
		match generate_proxy(&proxy_mut_name, true, &struct_name, &fields) {
			Ok(ts) => output.extend(ts),
			Err(e) => return e.to_compile_error().into(),
		}
	}

	output.into()
}
