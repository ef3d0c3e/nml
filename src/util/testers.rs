/// Recursively call field matched for each field
#[macro_export]
macro_rules! validate_ast_fields {
	($obj:expr, $idx:expr, $t:ty,) => {};
	// Parse a single field expression and continue
	($obj:expr, $idx:expr, $t:ty, $($field_tokens:tt)*) => {
		crate::validate_ast_parse_field!($obj, $idx, $t, [], $($field_tokens)*);
	};
}

/// Parse field expressions until first `==` encountered
#[macro_export]
macro_rules! validate_ast_parse_field {
	// Found `==` token, now we have the complete LHS expression
	($obj:expr, $idx:expr, $t:ty, [$($lhs:tt)*], == $value:expr, $($rest:tt)*) => {
		{
			let expected = $value;
			let actual = crate::validate_ast_evaluate_expr!($obj, $($lhs)*);
			assert_eq!(actual, expected,
				"Invalid field expression '{}' for {} at index {}, expected {:#?}, found {:#?}",
				stringify!($($lhs)*),
				stringify!($t),
				$idx,
				expected,
				actual
			);
		}

		// Continue parsing remaining fields
		crate::validate_ast_fields!($obj, $idx, $t, $($rest)*);
	};
	// Found `==` token without comma (last field)
	($obj:expr, $idx:expr, $t:ty, [$($lhs:tt)*], == $value:expr) => {
		{
			let expected = $value;
			let actual = crate::validate_ast_evaluate_expr!($obj, $($lhs)*);
			assert_eq!(*actual, expected,
				"Invalid field expression '{}' for {} at index {}, expected {:#?}, found {:#?}",
				stringify!($($lhs)*),
				stringify!($t),
				$idx,
				expected,
				actual
			);
		}
	};
	// Accumulate tokens until we find `==`
	($obj:expr, $idx:expr, $t:ty, [$($lhs:tt)*], $token:tt $($rest:tt)*) => {
		crate::validate_ast_parse_field!($obj, $idx, $t, [$($lhs)* $token], $($rest)*);
	};
}

// Helper macro to evaluate the left-hand side expression
#[macro_export]
macro_rules! validate_ast_evaluate_expr {
	// Simple method call
	($obj:expr, $method:ident ()) => {
		$obj.$method()
	};
	// Nested fields, then method call
	($obj:expr, $first:ident . $($middle:ident).* . $method:ident ()) => {
		crate::validate_ast_evaluate_expr!(&$obj.$first, $($middle).* . $method())
	};
	// Single field, then method call
	($obj:expr, $field:ident . $method:ident ()) => {
		$obj.$field.$method()
	};
	// Nested fields
	($obj:expr, $first:ident . $($rest:ident).+) => {
		crate::validate_ast_evaluate_expr!(&$obj.$first, $($rest).+)
	};
	// Single field
	($obj:expr, $field:ident) => {
		&$obj.$field
	};
}

#[macro_export]
macro_rules! validate_ast {
	($scope:expr, $idx:expr,) => {};
	($scope:expr, $idx:expr, $t:ty; $($tail:tt)*) => {{
		let elem = crate::unit::scope::ScopeAccessor::get_content($scope, $idx).unwrap();
		assert!(elem.downcast_ref::<$t>().is_some(), "Invalid element at index {}, expected: {} got: {}, in scope:\n{:#?}", $idx, stringify!($t), elem.element_name(), $scope);

		validate_ast!($scope, ($idx+1), $($tail)*);
	}};
	($scope:expr, $idx:expr, $t:ty { $($fields:tt)* }; $($tail:tt)*) => {{
		validate_ast!($scope, $idx, $t;);

		let elem = crate::unit::scope::ScopeAccessor::get_content($scope, $idx).unwrap();
		let obj = elem.downcast_ref::<$t>().unwrap();

		// Parse and validate fields using helper macro
		crate::validate_ast_fields!(obj, $idx, $t, $($fields)*);

		crate::validate_ast!($scope, ($idx+1), $($tail)*);
	}};
	($scope:expr, $idx:expr, $t:ty [ $( { $($ts:tt)* } )* ]; $($tail:tt)*) => {{
		validate_ast!($scope, $idx, $t;);

		let elem = crate::unit::scope::ScopeAccessor::get_content($scope, $idx).unwrap();
		let container = elem.as_container().expect("Expected container element");
		#[allow(unused)]
		let mut __i = 0;
		$(
			validate_ast!(&container.contained()[__i], 0, $($ts)*);
			__i += 1;
		)*
		validate_ast!($scope, ($idx+1), $($tail)*);
	}};
	($scope:expr, $idx:expr, $t:ty { $($fields:tt)* } [ $( { $($ts:tt)* } )* ]; $($tail:tt)*) => {{
		validate_ast!($scope, $idx, $t;);
		validate_ast!($scope, $idx, $t { $($field == $value)* };);
		validate_ast!($scope, $idx, $t [ $({$($ts)*})* ];);
		validate_ast!($scope, ($idx+1), $($tail)*);
	}};
}
