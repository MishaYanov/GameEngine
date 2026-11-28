//! Standalone Player residency and safe movement at an unavailable boundary.

use super::*;
use runtime::world_streaming::RegionOperation;

impl GameApp {
	pub(super) fn update_world_streaming(&mut self) -> Result<(), String> {
		let (Some(stream), Some(status), Some(focus), Some(project)) = (
			&mut self.world_streaming,
			self.editor.play_status(),
			self.editor.streaming_focus(),
			self.editor.project(),
		) else {
			self.world_streaming_status = None;
			if let Some(stream) = &mut self.world_streaming {
				stream.suspend();
			}
			return Ok(());
		};
		stream.enter_world(&status.run_id, status.scene_generation);
		if self.world_streaming_status.is_some() && self.input.just_pressed(Key::F8) {
			stream.retry_failed();
			self.world_streaming_status = None;
		}
		let resident = self.editor.resident_regions();
		let operation = match stream.poll(project, &status.scene_id, focus, &resident, |region| {
			self.editor.can_detach_region(region)
		}) {
			Ok(operation) => operation,
			Err(error) => {
				self.world_streaming_status = Some("Could not load region  /  F8: Retry".into());
				engine_logger::warn!(target: "world_streaming", "{error}");
				None
			}
		};
		if let Some(operation) = operation {
			let (region, attaching) = match &operation {
				RegionOperation::Attach { region, .. } => (region.clone(), true),
				RegionOperation::Detach { region } => (region.clone(), false),
			};
			let assets = self
				.asset_manager
				.as_mut()
				.ok_or("Asset manager is unavailable")?;
			let renderer = self.renderer.as_ref().ok_or("Renderer is unavailable")?;
			match self
				.editor
				.apply_world_region(operation, &mut self.scene, assets, renderer)
			{
				Ok(()) => self.world_streaming_status = None,
				Err(error) => {
					let message = if attaching {
						stream.reject(region);
						"Region unavailable  /  F8: Retry"
					} else {
						stream.defer_detach(region);
						"Could not unload region  /  F8: Retry"
					};
					self.world_streaming_status = Some(message.into());
					engine_logger::warn!(target: "world_streaming", "{error}");
				}
			}
		}
		let project = self
			.editor
			.project()
			.ok_or("Project configuration is unavailable")?;
		let blocked = stream.entry_blocked(
			project,
			&status.scene_id,
			focus,
			&self.editor.resident_regions(),
		);
		if blocked {
			self.world_streaming_status
				.get_or_insert_with(|| "Preparing next region".into());
		} else if self.world_streaming_status.as_deref() == Some("Preparing next region") {
			self.world_streaming_status = None;
		}
		Ok(())
	}
}
