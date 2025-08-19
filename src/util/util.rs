
/// Trait to get a `&T` for comparison.
pub trait AsRefForCompare<'a, T: ?Sized> {
	fn as_ref_for_compare(&'a self) -> &'a T;
}

// For owned T: just return a reference to self
impl<'a, T: ?Sized> AsRefForCompare<'a, T> for T {
	fn as_ref_for_compare(&'a self) -> &'a T {
		&self
	}
}

// For &T: return the inner reference
impl<'a, T: ?Sized> AsRefForCompare<'a, T> for &T {
	fn as_ref_for_compare(&'a self) -> &'a T {
		self
	}
}
