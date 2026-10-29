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
mod tests {
	use super::Rollback;
	use std::cell::RefCell;

	#[test]
	fn partial_construction_rolls_back_in_reverse_order_and_transfer_does_not_destroy() {
		let released = RefCell::new(Vec::new());
		let construct = |fail: bool| -> Result<Vec<u32>, ()> {
			let _parent = Rollback::new(1, |id| released.borrow_mut().push(id));
			let mut children = Rollback::new(vec![2], |ids: Vec<u32>| {
				released.borrow_mut().extend(ids.into_iter().rev());
			});
			children.value_mut().push(3);
			if fail {
				return Err(());
			}
			Ok(children.into_inner())
		};
		assert!(construct(true).is_err());
		assert_eq!(*released.borrow(), [3, 2, 1]);
		released.borrow_mut().clear();
		assert_eq!(construct(false).unwrap(), [2, 3]);
		assert_eq!(*released.borrow(), [1]);
	}
}
