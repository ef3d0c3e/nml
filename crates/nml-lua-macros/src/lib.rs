//! Proc macro for Lua bindings, carrying the docs and types used to generate LuaCATS declarations.
//!
//! The generated code refers to `::nml_lua`, so the macro is meant for the API functions of
//! the `nml-lua` crate.

use proc_macro::TokenStream;
use quote::format_ident;
use quote::quote;
use syn::Attribute;
use syn::Expr;
use syn::FnArg;
use syn::GenericArgument;
use syn::ItemFn;
use syn::Lit;
use syn::LitStr;
use syn::Meta;
use syn::Pat;
use syn::PathArguments;
use syn::ReturnType;
use syn::Type;
use syn::parse_macro_input;

/// Declares a function of the `nml` Lua table.
///
/// The function takes `lua: &mlua::Lua` first, then its Lua arguments, and returns
/// `mlua::Result<T>`. Its doc comment becomes the declaration's documentation; argument and
/// return types are translated to LuaCATS (`String` → `string`, `Option<T>` → `T?`, …),
/// unless overridden: `#[lua(type = "…")]` on an argument, `#[api(returns = "…")]` on the
/// function.
///
/// Generates, next to the function, a constant named after it in upper case: the
/// `ApiFunction` that registers it and describes it.
#[proc_macro_attribute]
pub fn api(attr: TokenStream, item: TokenStream) -> TokenStream {
	let mut function = parse_macro_input!(item as ItemFn);
	let returns_override = if attr.is_empty() {
		None
	} else {
		let meta = parse_macro_input!(attr as Meta);
		match string_value(&meta, "returns") {
			Ok(value) => Some(value),
			Err(error) => return error.to_compile_error().into(),
		}
	};
	match expand(&mut function, returns_override) {
		Ok(tokens) => tokens.into(),
		Err(error) => error.to_compile_error().into(),
	}
}

const LUA_KEYWORDS: &[&str] = &[
	"and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in",
	"local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

/// Exposes a struct to Lua as userdata, as the original's `auto_userdata` did, and
/// describes it for the LuaCATS declarations.
///
/// `#[userdata(name = "nml.Element")]` names the Lua class. Every named field becomes a Lua
/// field that reads a copy of its value and, unless marked `#[lua(readonly)]`, can be
/// assigned. `#[lua(skip)]` hides a field, and `#[lua(type = "…")]` overrides its LuaCATS
/// type. Doc comments become the declarations' documentation.
///
/// The struct is `Clone`-able field by field: a field's type implements `Clone`, `IntoLua`
/// and, if assignable, `FromLua`. Generates the `mlua::UserData` impl and the constant
/// `LUA_CLASS`, an `nml_lua::LuaClass`.
#[proc_macro_attribute]
pub fn userdata(attr: TokenStream, item: TokenStream) -> TokenStream {
	let meta = parse_macro_input!(attr as Meta);
	let mut item = parse_macro_input!(item as syn::ItemStruct);
	let name = match string_value(&meta, "name") {
		Ok(name) => name,
		Err(error) => return error.to_compile_error().into(),
	};
	match expand_userdata(&mut item, name) {
		Ok(tokens) => tokens.into(),
		Err(error) => error.to_compile_error().into(),
	}
}

fn expand_userdata(
	item: &mut syn::ItemStruct,
	class: String,
) -> syn::Result<proc_macro2::TokenStream> {
	let ident = item.ident.clone();
	let class_doc = doc(&item.attrs);
	let syn::Fields::Named(fields) = &mut item.fields else {
		return Err(syn::Error::new_spanned(
			&item.ident,
			"#[userdata] needs named fields",
		));
	};
	let mut accessors = Vec::new();
	let mut described = Vec::new();
	for field in fields.named.iter_mut() {
		let mut readonly = false;
		let mut skip = false;
		let mut ty_override = None;
		let mut kept = Vec::new();
		for attribute in field.attrs.drain(..) {
			if !attribute.path().is_ident("lua") {
				kept.push(attribute);
				continue;
			}
			attribute.parse_args_with(|input: syn::parse::ParseStream| {
				while !input.is_empty() {
					if input.peek(syn::Token![type]) {
						input.parse::<syn::Token![type]>()?;
						input.parse::<syn::Token![=]>()?;
						ty_override = Some(input.parse::<LitStr>()?.value());
					} else {
						let word: syn::Ident = input.parse()?;
						match word.to_string().as_str() {
							"readonly" => readonly = true,
							"skip" => skip = true,
							_ => {
								return Err(syn::Error::new_spanned(
									word,
									"expected `readonly`, `skip` or `type = \"…\"`",
								));
							}
						}
					}
					if !input.is_empty() {
						input.parse::<syn::Token![,]>()?;
					}
				}
				Ok(())
			})?;
		}
		field.attrs = kept;
		if skip {
			continue;
		}
		let field_ident = field.ident.clone().unwrap();
		let field_name = LitStr::new(&field_ident.to_string(), field_ident.span());
		let ty = &field.ty;
		let lua_ty = match ty_override {
			Some(ty) => ty,
			None => lua_type(ty)?,
		};
		let field_doc = doc(&field.attrs);
		described.push(quote! {
			::nml_lua::LuaField { name: #field_name, ty: #lua_ty, doc: #field_doc, readonly: #readonly }
		});
		accessors.push(quote! {
			fields.add_field_method_get(#field_name, |_, this| Ok(::std::clone::Clone::clone(&this.#field_ident)));
		});
		if !readonly {
			accessors.push(quote! {
				fields.add_field_method_set(#field_name, |_, this, value: #ty| {
					this.#field_ident = value;
					Ok(())
				});
			});
		}
	}
	Ok(quote! {
		#item

		impl ::mlua::UserData for #ident {
			fn add_fields<F: ::mlua::UserDataFields<Self>>(fields: &mut F) {
				#(#accessors)*
			}
		}

		impl #ident {
			/// The description of the Lua class, for the LuaCATS declarations.
			pub(crate) const LUA_CLASS: ::nml_lua::LuaClass = ::nml_lua::LuaClass {
				name: #class,
				doc: #class_doc,
				fields: &[#(#described),*],
			};
		}
	})
}

/// The string of `name = "…"`.
fn string_value(meta: &Meta, name: &str) -> syn::Result<String> {
	match meta {
		Meta::NameValue(pair) if pair.path.is_ident(name) => match &pair.value {
			Expr::Lit(literal) => match &literal.lit {
				Lit::Str(value) => Ok(value.value()),
				other => Err(syn::Error::new_spanned(other, "expected a string")),
			},
			other => Err(syn::Error::new_spanned(other, "expected a string")),
		},
		other => Err(syn::Error::new_spanned(
			other,
			format!("expected `{name} = \"…\"`"),
		)),
	}
}

/// The lines of the doc comments, without their leading space.
fn doc(attributes: &[Attribute]) -> String {
	let mut lines = Vec::new();
	for attribute in attributes {
		if let Meta::NameValue(pair) = &attribute.meta
			&& pair.path.is_ident("doc")
			&& let Expr::Lit(literal) = &pair.value
			&& let Lit::Str(text) = &literal.lit
		{
			let text = text.value();
			lines.push(text.strip_prefix(' ').unwrap_or(&text).to_string());
		}
	}
	lines.join("\n")
}

/// The LuaCATS type of a Rust type.
fn lua_type(ty: &Type) -> syn::Result<String> {
	let Type::Path(path) = ty else {
		if let Type::Tuple(tuple) = ty
			&& tuple.elems.is_empty()
		{
			return Ok("nil".into());
		}
		return Err(syn::Error::new_spanned(
			ty,
			"no LuaCATS type for this type: add #[lua(type = \"…\")]",
		));
	};
	let segment = path.path.segments.last().unwrap();
	let inner = || -> syn::Result<String> {
		match &segment.arguments {
			PathArguments::AngleBracketed(arguments) => match arguments.args.first() {
				Some(GenericArgument::Type(inner)) => lua_type(inner),
				_ => Err(syn::Error::new_spanned(segment, "expected a type argument")),
			},
			_ => Err(syn::Error::new_spanned(segment, "expected a type argument")),
		}
	};
	Ok(match segment.ident.to_string().as_str() {
		"String" => "string".into(),
		"bool" => "boolean".into(),
		"i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize" => {
			"integer".into()
		}
		"f32" | "f64" => "number".into(),
		"Function" => "function".into(),
		"Table" => "table".into(),
		"Value" => "any".into(),
		"Option" => format!("{}?", inner()?),
		"Vec" => format!("{}[]", inner()?),
		_ => {
			return Err(syn::Error::new_spanned(
				ty,
				"no LuaCATS type for this type: add #[lua(type = \"…\")]",
			));
		}
	})
}

/// The `T` of `mlua::Result<T>`.
fn result_type(output: &ReturnType) -> syn::Result<&Type> {
	if let ReturnType::Type(_, ty) = output
		&& let Type::Path(path) = &**ty
		&& let Some(segment) = path.path.segments.last()
		&& segment.ident == "Result"
		&& let PathArguments::AngleBracketed(arguments) = &segment.arguments
		&& let Some(GenericArgument::Type(inner)) = arguments.args.first()
	{
		return Ok(inner);
	}
	Err(syn::Error::new_spanned(
		output,
		"an API function returns `mlua::Result<T>`",
	))
}

fn expand(
	function: &mut ItemFn,
	returns_override: Option<String>,
) -> syn::Result<proc_macro2::TokenStream> {
	let name = &function.sig.ident;
	let lua_name = name.to_string();
	if LUA_KEYWORDS.contains(&lua_name.as_str()) {
		return Err(syn::Error::new_spanned(
			name,
			format!("`{lua_name}` is a Lua keyword: `nml.{lua_name}` cannot be called"),
		));
	}
	let doc = doc(&function.attrs);

	let mut inputs = function.sig.inputs.iter_mut();
	match inputs.next() {
		Some(FnArg::Typed(_)) => {}
		_ => {
			return Err(syn::Error::new_spanned(
				&function.sig,
				"an API function takes `lua: &mlua::Lua` first",
			));
		}
	}
	let mut params = Vec::new();
	let mut idents = Vec::new();
	let mut types = Vec::new();
	for input in inputs {
		let FnArg::Typed(typed) = input else {
			return Err(syn::Error::new_spanned(input, "unexpected `self`"));
		};
		let Pat::Ident(ident) = &*typed.pat else {
			return Err(syn::Error::new_spanned(&typed.pat, "expected a name"));
		};
		let mut lua_type_override = None;
		let mut kept = Vec::new();
		for attribute in typed.attrs.drain(..) {
			if attribute.path().is_ident("lua") {
				// `type` is a keyword, so this is parsed by hand: `#[lua(type = "…")]`.
				let ty = attribute.parse_args_with(|input: syn::parse::ParseStream| {
					input.parse::<syn::Token![type]>()?;
					input.parse::<syn::Token![=]>()?;
					input.parse::<LitStr>()
				})?;
				lua_type_override = Some(ty.value());
			} else {
				kept.push(attribute);
			}
		}
		typed.attrs = kept;
		let ty = match lua_type_override {
			Some(ty) => ty,
			None => lua_type(&typed.ty)?,
		};
		let param = ident.ident.to_string();
		params.push(quote! { (#param, #ty) });
		idents.push(ident.ident.clone());
		types.push((*typed.ty).clone());
	}

	let returns = match returns_override {
		Some(returns) => returns,
		None => {
			let ty = result_type(&function.sig.output)?;
			lua_type(ty).map_err(|_| {
				syn::Error::new_spanned(
					ty,
					"no LuaCATS type for this type: add #[api(returns = \"…\")]",
				)
			})?
		}
	};
	let returns = if returns == "nil" {
		quote! { None }
	} else {
		quote! { Some(#returns) }
	};
	let constant = format_ident!("{}", lua_name.to_uppercase());
	let lua_name = LitStr::new(&lua_name, name.span());
	Ok(quote! {
		#function

		#[doc = concat!("The `nml.", #lua_name, "` function.")]
		pub(crate) const #constant: ::nml_lua::ApiFunction = ::nml_lua::ApiFunction {
			name: #lua_name,
			doc: #doc,
			params: &[#(#params),*],
			returns: #returns,
			register: |lua, table| {
				let function = lua.create_function(|lua, (#(#idents,)*): (#(#types,)*)| {
					#name(lua, #(#idents),*)
				})?;
				table.set(#lua_name, function)
			},
		};
	})
}
