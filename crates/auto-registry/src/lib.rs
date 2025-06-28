#![feature(proc_macro_span)]
use std::cell::RefCell;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::collections::HashMap;

use proc_macro::TokenStream;
use quote::quote;
use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::parse_macro_input;
use syn::ItemStruct;


static REGISTRY: LazyLock<Mutex<HashMap<String, Vec<String>>>> = LazyLock::new(|| Mutex::new(HashMap::default()));

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
/// # Example
///
/// ```
/// #[auto_registry::auto_registry(registry = "listeners")]
/// struct KeyboardListener { ... }
/// ```
/// This will register `KeyboardListener` to the `listeners` registry.
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

	let mut reg = REGISTRY.lock().unwrap();
	if let Some(ref mut vec) = reg.get_mut(args.registry.value().as_str()) {
		vec.push(path);
	} else {
		reg.insert(args.registry.value(), vec![path]);
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
	/// The collector macro, takes all constructed items and processes them
	collector: Option<syn::Expr>,
	/// The maper macro, maps types to expressions
	mapper: Option<syn::Expr>,
	/// The name of the output macro
	output: syn::Ident,
}

/// Parser for [`GenerateRegistryArgs`]
impl Parse for GenerateRegistryArgs {
	fn parse(input: ParseStream) -> syn::Result<Self> {
		let mut registry = None;
		let mut collector = None;
		let mut mapper = None;
		let mut output = None;
		loop {
			let key: syn::Ident = input.parse()?;
			input.parse::<syn::Token![=]>()?;

			match key.to_string().as_str() {
				"registry" => registry = Some(input.parse()?),
				"collector" => collector = Some(input.parse()?),
				"mapper" => mapper = Some(input.parse()?),
				"output" => output = Some(input.parse()?),
				_ => {
					return Err(syn::Error::new(
						key.span(),
						format!(
							"Unknown attribute `{}`, excepted `registry`, `collector`, `mapper` or `output`",
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
		} else if output.is_none() {
			return Err(syn::Error::new(
				input.span(),
				"Missing required attribute `output`".to_string(),
			));
		} else if collector.is_none() && mapper.is_none() {
			return Err(syn::Error::new(
				input.span(),
				"Macro requires that either `collector` or `mapper` be set".to_string(),
			));
		}

		Ok(GenerateRegistryArgs {
			registry: registry.unwrap(),
			collector,
			mapper,
			output: output.unwrap(),
		})
	}
}

/// The proc macro that generates the function to build the registry
///
/// # Attributes
///  - registry: (String) Name of the registry to generate
///  - collector: (Optional Macro) A macro that will take all the newly constructed
///           objects comma-separated and create the resulting expression
///  - mapper: (Optional Macro) A macro that will map each registered types to
///            an expression. By default `$type::default()` will be called.
///  - output: (Identifier) The generated macro to get access to all registered
///            values. Calling to this macro is what actually generates the values
///
/// Note: Using `mapper` and `collector` will pass the results of calling `mapper`
/// on all types in the registry to `collector`
///
/// # Example
///
/// Basic example
/// ```
/// #[auto_registry::auto_registry(registry = "listeners")]
/// #[derive(Default)]
/// struct KeyboardListener { ... }
///
/// #[auto_registry::auto_registry(registry = "listeners")]
/// #[derive(Default)]
/// struct MouseListener { ... }
///
/// macro_rules! collect_listeners { // Collects to a Vec<Box<dyn Listener>>
/// 	( $($construct:expr);+ $(;)? ) => {{ // Macro must accepts `;`-separated arguments
/// 		vec![$(Box::new($construct) as Box<dyn Listener + Send + Sync>,)+]
/// 	}};
/// }
///
/// #[auto_registry::generate_registry(registry = "listeners", collector = collect_listeners, output = get_listeners)]
///
/// fn main()
/// {
/// 	// All listeners will be initialized by calling to `::default()`
/// 	let listeners = get_listeners!();
/// }
/// ```
///
/// Example using `mapper`
/// ```
/// #[auto_registry::auto_registry(registry = "listeners")]
/// #[derive(Default)]
/// struct KeyboardListener { ... }
///
/// #[auto_registry::auto_registry(registry = "listeners")]
/// #[derive(Default)]
/// struct MouseListener { ... }
///
/// // Some global variable that will hold out registered listeners
/// static LISTENERS: LazyLock<Mutex<Vec<Box<dyn Listener + Send + Sync>>>> = LazyLock::new(|| Mutex::new(Vec::default()));
///
/// macro_rules! register_listener { // Register a single listener
/// 	($t:ty) => {{
/// 		let mut listeners = LISTENERS.lock();
/// 		listeners
/// 			.unwrap()
/// 			.push(Box::new(<$t>::default()) as Box<dyn Listener + Send + Sync>);
/// 	}};
/// }
///
/// #[auto_registry::generate_registry(registry = "listeners", mapper = register_listener, output = register_all_listeners)]
///
/// fn main()
/// {
/// 	register_all_listeners!();
/// }
/// ```
#[proc_macro_attribute]
pub fn generate_registry(attr: TokenStream, input: TokenStream) -> TokenStream {
	let args = parse_macro_input!(attr as GenerateRegistryArgs);
	let reg = REGISTRY.lock().unwrap();

	let mut stream = proc_macro2::TokenStream::new();
	if let Some(names) = reg.get(args.registry.value().as_str()) {
		for name in names {
			let struct_name: proc_macro2::TokenStream = name.parse().unwrap();
			if let Some(ref mapper) = args.mapper
			{
				stream.extend(quote::quote_spanned!(proc_macro2::Span::call_site() =>
					#mapper!(#struct_name);
				));
			}
			else
			{
				stream.extend(quote::quote_spanned!(proc_macro2::Span::call_site() =>
					#struct_name::default(),
				));
			}
		}
	} else {
		panic!(
			"Unable to find registry item with key=`{}`",
			args.registry.value()
		);
	}

	let rest: proc_macro2::TokenStream = input.into();
	let output = args.output;

	if let Some(collector) = args.collector
	{
		quote! {
			macro_rules! #output  {
				() => { #collector!(#stream); };
			}
			#rest
		}
	}
	else
	{
		quote! {
			macro_rules! #output  {
				() => { #stream };
			}
			#rest
		}
	}
	.into()
}
