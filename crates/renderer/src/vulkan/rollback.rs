/// Owns a partially constructed resource until the final owner accepts it.
pub(super) struct Rollback<T, F: FnOnce(T)> {
	value: Option<T>,
	cleanup: Option<F>,
}

impl<T, F: FnOnce(T)> Rollback<T, F> {
	pub(super) fn new(value: T, cleanup: F) -> Self {
		Self {
			value: Some(value),
			cleanup: Some(cleanup),
		}
	}

	pub(super) fn value_mut(&mut self) -> &mut T {
		self.value
			.as_mut()
			.expect("resource has not been transferred")
	}

	pub(super) fn into_inner(mut self) -> T {
		self.value
			.take()
			.expect("resource has not been transferred")
	}
}

impl<T, F: FnOnce(T)> Drop for Rollback<T, F> {
	fn drop(&mut self) {
		if let Some(value) = self.value.take() {
			(self.cleanup.take().expect("cleanup exists"))(value);
		}
	}
}

#[cfg(test)]
#[path = "../../tests/unit/vulkan/rollback.rs"]
mod tests;
