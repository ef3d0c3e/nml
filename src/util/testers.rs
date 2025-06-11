
#[macro_export]
macro_rules! validate_ast {
	($scope:expr, $idx:expr,) => {};
	($scope:expr, $idx:expr, $t:ty; $($tail:tt)*) => {{
		let elem = crate::unit::scope::ScopeAccessor::get_content($scope, $idx).unwrap();
		assert!(elem.downcast_ref::<$t>().is_some(), "Invalid element at index {}, expected: {} got: {}, in scope:\n{:#?}", $idx, stringify!($t), elem.element_name(), $scope);

		validate_ast!($scope, ($idx+1), $($tail)*);
	}};
	($scope:expr, $idx:expr, $t:ty { $($field:ident == $value:expr),* }; $($tail:tt)*) => {{
		validate_ast!($scope, $idx, $t;);

		let elem = crate::unit::scope::ScopeAccessor::get_content($scope, $idx).unwrap();
		$(
			let val = &elem.downcast_ref::<$t>().unwrap().$field;
			assert!(*val == $value, "Invalid field {} for {} at index {}, expected {:#?}, found {:#?}",
				stringify!($field),
				stringify!($t),
				$idx,
				$value,
				val);
		)*

		validate_ast!($scope, ($idx+1), $($tail)*);
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
	($scope:expr, $idx:expr, $t:ty { $($field:ident == $value:expr),* } [ $( { $($ts:tt)* } )* ]; $($tail:tt)*) => {{
		validate_ast!($scope, $idx, $t;);
		validate_ast!($scope, $idx, $t { $($field == $value)* };);
		validate_ast!($scope, $idx, $t [ $({$($ts)*})* ];);
		validate_ast!($scope, ($idx+1), $($tail)*);
	}};
}
