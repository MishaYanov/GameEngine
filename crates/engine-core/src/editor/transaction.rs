use super::*;

impl EditorSession {
	pub(super) fn begin_transaction(&mut self, label: String) -> Result<(), String> {
		self.require_edit()?;
		self.require_no_transaction()?;
		if label.trim().is_empty() || label.len() > 256 || label.chars().any(char::is_control) {
			return Err("An edit label needs 1–256 bytes".into());
		}
		self.transaction = Some(Transaction {
			label,
			before: Arc::clone(&self.data),
			project: Arc::clone(&self.project),
			selection: self.selection.clone(),
		});
		self.bump();
		Ok(())
	}

	pub(super) fn commit_transaction(&mut self) -> Result<(), String> {
		self.require_edit()?;
		let transaction = self
			.transaction
			.as_ref()
			.ok_or("No grouped edit is active")?;
		let patch = Patch::between(
			&transaction.before,
			&self.data,
			&transaction.project,
			&self.project,
			&transaction.selection,
			&self.selection,
			transaction.label.clone(),
		)?
		.in_scene(self.active_scene_id.clone());
		self.history.record(patch)?;
		self.transaction = None;
		self.bump();
		Ok(())
	}

	pub(super) fn cancel_transaction(&mut self) -> Result<(), String> {
		self.require_edit()?;
		let transaction = self.transaction.take().ok_or("No grouped edit is active")?;
		self.data = transaction.before;
		self.project = transaction.project;
		self.selection = transaction.selection;
		self.scene_revision += 1;
		self.bump();
		Ok(())
	}
}
