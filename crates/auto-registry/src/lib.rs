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
use syn::ItemStruct;

lazy_static! {
	/// The registry, each key corresponds to an identifier that needs to be
	/// valid in the context of the [`genegenerate_registry`] macro.
	static ref REGISTRY: Mutex<RefCell<HashMap<String, Vec<String>>>> =
		Mutex::new(RefCell::new(HashMap::new()));
}

/// Arguments for the [`auto_registry`] proc macro
struct AutoRegistryArgs {
	/// The registry name
	registry: syn::LitStr,
	/// The absolute path to the struct, if not specified the macro will try
	/// to automatically infer the full path.
	path: Option<syn::LitStr>,
}

/// Parser for [`AutoRegistryArgs`]
impl Parse for AutoRegistryArgs {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		let mut registry = None;
		let mut path = None;
		loop {
			let key: syn::Ident = input.parse()?;
			input.parse::<syn::Token![=]>()?;
			let value: syn::LitStr = input.parse()?;

			match key.to_string().as_str() {
				"registry" => registry = Some(value),
				"path" => path = Some(value),
				_ => {
					return Err(syn::Error::new(
						key.span(),
						format!(
							"Unknown attribute `{}`, excepted `registry` or `path`",
							key.to_string()
						),
					))
				}
			}
			if input.is_empty() {
				break;
			}
			input.parse::<syn::Token![,]>()?;
		}

		if registry.is_none() {
			return Err(syn::Error::new(
				input.span(),
				"Missing required attribute `registry`".to_string(),
			));
		}

		Ok(AutoRegistryArgs {
			registry: registry.unwrap(),
			path,
		})
	}
}

/// The proc macro used on a struct to add it to the registry
///
/// # Attributes
///  - registry: (String) Name of the registry to collect the struct into
///  - path: (Optional String) The crate path in which the struct is located
///          If left empty, the path will be try to be automatically-deduced
///
/// # Note
///
/// Due to a lacking implementation of `proc_macro_span` in rust-analyzer,
/// it is highly advised the set the `path` attribute when using this macro.
/// See https://github.com/rust-lang/rust-analyzer/issues/15950
#[proc_macro_attribute]
pub fn auto_registry(attr: TokenStream, input: TokenStream) -> TokenStream {
	let args = parse_macro_input!(attr as AutoRegistryArgs);
	let input = parse_macro_input!(input as ItemStruct);

	let ident = &input.ident;

	let path = if let Some(path) = args.path {
		let value = path.value();
		if value.is_empty() {
			value
		} else {
			format!("{}::{}", value, ident.to_string().as_str())
		}
	} else {
		// Attempt to get the path in a hacky way in case the path wasn't
		// specified as an attribute to the macro
		let path = match input
			.ident
			.span()
			.unwrap()
			.source_file()
			.path()
			.canonicalize()
		{
			Ok(path) => path,
			Err(e) => {
				return syn::Error::new(
					input.ident.span(),
					format!("Failed to canonicalize path: {}", e),
				)
				.to_compile_error()
				.into();
			}
		};

		let crate_path = std::env::var("CARGO_MANIFEST_DIR").unwrap();
		let relative_path = path.strip_prefix(&crate_path).unwrap();
		let relative_path_str = relative_path.to_string_lossy();
		// Remove the first path component e.g "src/"
		let pos = if let Some(pos) = relative_path_str.find("/") {
			pos + 1
		} else {
			0
		};

		let module_path = relative_path_str
			.split_at(pos)
			.1
			.strip_suffix(".rs")
			.unwrap()
			.replace("/", "::");

		if module_path.is_empty() {
			format!("crate::{}", ident.to_string())
		} else {
			format!("crate::{module_path}::{}", ident.to_string())
		}
	};

	let reg_mtx = REGISTRY.lock().unwrap();
	let mut reg_borrow = reg_mtx.borrow_mut();
	if let Some(ref mut vec) = reg_borrow.get_mut(args.registry.value().as_str()) {
		vec.push(path);
	} else {
		reg_borrow.insert(args.registry.value(), vec![path]);
	}

	quote! {
		#input
	}
	.into()
}

/// Arguments for the [`generate_registry`] proc macro
struct GenerateRegistryArgs {
	/// The registry name
	registry: syn::LitStr,
	/// The target, i.e the generated function name
	target: syn::Ident,
	/// The maker macro, takes all constructed items and processes them
	maker: syn::Expr,
	/// The return type for the function
	return_type: syn::Type,
}

/// Parser for [`GenerateRegistryArgs`]
impl Parse for GenerateRegistryArgs {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		let mut registry = None;
		let mut target = None;
		let mut maker = None;
		let mut return_type = None;
		loop {
			let key: syn::Ident = input.parse()?;
			input.parse::<syn::Token![=]>()?;

			match key.to_string().as_str() {
				"registry" => registry = Some(input.parse()?),
				"target" => target = Some(input.parse()?),
				"maker" => maker = Some(input.parse()?),
				"return_type" => return_type = Some(input.parse()?),
				_ => {
					return Err(syn::Error::new(
						key.span(),
						format!(
							"Unknown attribute `{}`, excepted `registry` or `target`",
							key.to_string()
						),
					))
				}
			}
			if input.is_empty() {
				break;
			}
			input.parse::<syn::Token![,]>()?;
		}

		if registry.is_none() {
			return Err(syn::Error::new(
				input.span(),
				"Missing required attribute `registry`".to_string(),
			));
		} else if target.is_none() {
			return Err(syn::Error::new(
				input.span(),
				"Missing required attribute `target`".to_string(),
			));
		} else if maker.is_none() {
			return Err(syn::Error::new(
				input.span(),
				"Missing required attribute `maker`".to_string(),
			));
		} else if return_type.is_none() {
			return Err(syn::Error::new(
				input.span(),
				"Missing required attribute `return_type`".to_string(),
			));
		}

		Ok(GenerateRegistryArgs {
			registry: registry.unwrap(),
			target: target.unwrap(),
			maker: maker.unwrap(),
			return_type: return_type.unwrap(),
		})
	}
}

/// The proc macro that generates the function to build the registry
///
/// # Attributes
///  - registry: (String) Name of the registry to generate
///  - target: (Identifier) Name of the resulting function
///  - maker: (Macro) A macro that will take all the newly constructed objects
///           comma-separated and create the resulting expression
///  - return_type: (Type) The return type of the generated function.
///                 Must match the type of the macro invocation
///
/// # Example
/// ```
/// macro_rules! create_listeners {
/// 	( $($construct:expr),+ $(,)? ) => {{
/// 		vec![$(Box::new($construct) as Box<dyn Listener>,)+]
/// 	}};
/// }
/// #[generate_registry(
/// 		registry = "listeners",
/// 		target = build_listeners,
/// 		return_type = Vec<Box<dyn Listener>>,
/// 		maker = create_listeners)]
///
/// fn main()
/// {
/// 	let all_listeners : Vec<Box<dyn Listener>> = build_listeners();
/// }
/// ```
#[proc_macro_attribute]
pub fn generate_registry(attr: TokenStream, input: TokenStream) -> TokenStream {
	let args = parse_macro_input!(attr as GenerateRegistryArgs);
	let reg_mtx = REGISTRY.lock().unwrap();

	let mut stream = proc_macro2::TokenStream::new();
	if let Some(names) = reg_mtx.borrow().get(args.registry.value().as_str()) {
		for name in names {
			let struct_name: proc_macro2::TokenStream = name.parse().unwrap();
			stream.extend(quote::quote_spanned!(proc_macro2::Span::call_site() =>
				#struct_name::new(),
			));
		}
	} else {
		panic!(
			"Unable to find registry item with key=`{}`",
			args.registry.value()
		);
	}

	let function = args.target;
	let return_type = args.return_type;
	let maker = args.maker;

	let rest: proc_macro2::TokenStream = input.into();
	quote! {
		fn #function() -> #return_type {
			#maker!(
				#stream
			)
		}
		#rest
	}
	.into()
}
