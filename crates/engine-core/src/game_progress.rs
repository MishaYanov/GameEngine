use std::any::{Any, TypeId};

pub const MAX_PROGRESS_BYTES: usize = 64 * 1024;
pub const MAX_PROGRESS_ACTION_BYTES: usize = 4096;
pub const MAX_PROGRESS_ACTIONS: usize = 64;

/// Trusted native game data. Clone must produce an independent candidate;
/// validation and application must have no external side effects. Byte counts
/// include owned heap storage. The engine does not sandbox native game code.
pub trait GameProgress: Clone + PartialEq + Send + Sync + 'static {
	type Action: Send + Sync + 'static;
	fn validate(&self) -> Result<(), String>;
	fn retained_bytes(&self) -> usize;
	fn action_bytes(action: &Self::Action) -> usize;
	fn apply(&mut self, action: &Self::Action) -> Result<(), String>;
}

pub struct ProgressAction {
	state_type: TypeId,
	value: Box<dyn Any + Send + Sync>,
}

impl ProgressAction {
	pub fn new<P: GameProgress>(action: P::Action) -> Result<Self, String> {
		if P::action_bytes(&action) > MAX_PROGRESS_ACTION_BYTES {
			return Err("Game progress action exceeds its byte budget".into());
		}
		Ok(Self {
			state_type: TypeId::of::<P>(),
			value: Box::new(action),
		})
	}
}

trait ErasedProgress: Send + Sync {
	fn as_any(&self) -> &dyn Any;
	fn clone_value(&self) -> Box<dyn ErasedProgress>;
	fn equals(&self, other: &dyn ErasedProgress) -> bool;
	fn apply_action(&mut self, action: &ProgressAction) -> Result<(), String>;
}

fn validate_progress<P: GameProgress>(value: &P) -> Result<(), String> {
	if value.retained_bytes() > MAX_PROGRESS_BYTES {
		return Err("Game progress exceeds its byte budget".into());
	}
	value.validate()
}

impl<P: GameProgress> ErasedProgress for P {
	fn as_any(&self) -> &dyn Any {
		self
	}
	fn clone_value(&self) -> Box<dyn ErasedProgress> {
		Box::new(self.clone())
	}
	fn equals(&self, other: &dyn ErasedProgress) -> bool {
		other
			.as_any()
			.downcast_ref::<P>()
			.is_some_and(|value| value == self)
	}
	fn apply_action(&mut self, action: &ProgressAction) -> Result<(), String> {
		if action.state_type != TypeId::of::<P>() {
			return Err("Game progress action belongs to another progress type".into());
		}
		let value = action
			.value
			.downcast_ref::<P::Action>()
			.ok_or("Game progress action type does not match")?;
		self.apply(value)?;
		validate_progress(self)
	}
}

#[derive(Clone, Copy)]
pub struct ProgressView<'a> {
	value: &'a dyn Any,
	revision: u64,
}

impl<'a> ProgressView<'a> {
	pub fn get<P: GameProgress>(&self) -> Option<&'a P> {
		self.value.downcast_ref()
	}
	pub fn revision(&self) -> u64 {
		self.revision
	}
}

/// One game-owned progress value. Mutation is available only as atomic actions.
pub struct ProgressState {
	value: Box<dyn ErasedProgress>,
	revision: u64,
}

impl ProgressState {
	pub fn new<P: GameProgress>(value: P) -> Result<Self, String> {
		validate_progress(&value)?;
		Ok(Self {
			value: Box::new(value),
			revision: 0,
		})
	}
	pub fn empty() -> Self {
		Self {
			value: Box::new(EmptyProgress),
			revision: 0,
		}
	}
	pub fn view(&self) -> ProgressView<'_> {
		ProgressView {
			value: self.value.as_any(),
			revision: self.revision,
		}
	}
	pub fn apply_batch(&mut self, actions: &[ProgressAction]) -> Result<bool, String> {
		if actions.len() > MAX_PROGRESS_ACTIONS {
			return Err("Game progress action budget exceeded".into());
		}
		let mut transaction = self.transaction();
		for action in actions {
			transaction.apply(action)?;
		}
		Ok(transaction.commit())
	}
	pub(crate) fn transaction(&mut self) -> ProgressTransaction<'_> {
		ProgressTransaction {
			committed: self,
			candidate: None,
			actions: 0,
		}
	}
}

pub(crate) struct ProgressTransaction<'a> {
	committed: &'a mut ProgressState,
	candidate: Option<Box<dyn ErasedProgress>>,
	actions: usize,
}

impl ProgressTransaction<'_> {
	pub fn view(&self) -> ProgressView<'_> {
		ProgressView {
			value: self
				.candidate
				.as_deref()
				.unwrap_or(self.committed.value.as_ref())
				.as_any(),
			revision: self.committed.revision,
		}
	}
	pub fn apply(&mut self, action: &ProgressAction) -> Result<(), String> {
		if self.actions == MAX_PROGRESS_ACTIONS {
			return Err("Game progress action budget exceeded".into());
		}
		if self.committed.revision == u64::MAX {
			return Err("Game progress revision is exhausted".into());
		}
		self.actions += 1;
		let candidate = self
			.candidate
			.get_or_insert_with(|| self.committed.value.clone_value());
		candidate.apply_action(action)
	}
	pub fn commit(self) -> bool {
		if let Some(candidate) = self.candidate
			&& !candidate.equals(self.committed.value.as_ref())
		{
			self.committed.value = candidate;
			self.committed.revision += 1;
			true
		} else {
			false
		}
	}
}

#[derive(Clone, PartialEq)]
struct EmptyProgress;
impl GameProgress for EmptyProgress {
	type Action = ();
	fn validate(&self) -> Result<(), String> {
		Ok(())
	}
	fn retained_bytes(&self) -> usize {
		0
	}
	fn action_bytes(_: &()) -> usize {
		0
	}
	fn apply(&mut self, _: &()) -> Result<(), String> {
		Ok(())
	}
}
