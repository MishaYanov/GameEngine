//! Bounded region adoption by the native Editor Play viewport.

use super::*;
use runtime::world_streaming::{RegionOperation, WorldStreaming};

impl Worker {
	pub(super) fn needs_world_streaming_poll(&self, workspace: &EditorWorkspace) -> bool {
		let session = workspace.backend.session();
		let Some(status) = session.play_status() else {
			return false;
		};
		session.streaming_focus().is_some()
			&& session
			.project()
			.regions_for(&status.scene_id)
			.next()
			.is_some()
			&& self.world_streaming.as_ref().is_none_or(|(owner, stream)| {
			*owner != workspace.generation
				|| !stream.has_scope(&status.run_id, status.scene_generation)
				|| stream.waiting()
		})
	}
	pub(super) fn update_world_streaming(&mut self) -> Result<(), String> {
		let staged = {
			let mut shared = self
				.shared
				.lock()
				.map_err(|_| "Editor document lock is poisoned")?;
			let Some(workspace) = shared.as_mut() else {
				self.world_streaming = None;
				return Ok(());
			};
			let session = workspace.backend.session();
			let (Some(status), Some(focus), Some(gpu)) = (
				session.play_status(),
				session.streaming_focus(),
				self.gpu.as_ref(),
			) else {
				self.world_streaming = None;
				return Ok(());
			};
			if workspace.backend.pending_scene_transition().is_some()
				|| session
				.project()
				.regions_for(&status.scene_id)
				.next()
				.is_none() || gpu.generation != workspace.generation
			{
				self.world_streaming = None;
				return Ok(());
			}
			if self
				.world_streaming
				.as_ref()
				.is_none_or(|(generation, _)| *generation != workspace.generation)
			{
				self.world_streaming =
					Some((workspace.generation, WorldStreaming::new(&gpu.asset_root)?));
			}
			let (_, stream) = self.world_streaming.as_mut().expect("current world worker");
			stream.enter_world(&status.run_id, status.scene_generation);
			let resident = session.resident_regions();
			let operation = match stream.poll(
				session.project(),
				&status.scene_id,
				focus,
				&resident,
				|region| session.can_detach_region(region),
			) {
				Ok(Some(operation)) => operation,
				Ok(None) => return Ok(()),
				Err(error) => {
					self.interaction.report(error);
					return Ok(());
				}
			};
			let (prepared, region, attaching) = match operation {
				RegionOperation::Attach { region, document } => (
					session.prepare_region_attach(&region, document),
					region,
					true,
				),
				RegionOperation::Detach { region } => {
					(session.prepare_region_detach(&region), region, false)
				}
			};
			(workspace.generation, prepared, region, attaching)
		};
		let (generation, prepared, region, attaching) = staged;
		let result = prepared.and_then(|mut prepared| {
			let gpu = self.gpu.as_mut().ok_or("Native viewport is unavailable")?;
			let mut candidate =
				gpu.scene
					.prepare_region_geometry(&mut prepared, &mut gpu.assets, &gpu.renderer)?;

			let mut shared = self
				.shared
				.lock()
				.map_err(|_| "Editor document lock is poisoned")?;
			let Some(workspace) = shared
				.as_mut()
				.filter(|workspace| workspace.generation == generation)
			else {
				return Ok(());
			};
			if let Some(bank) = workspace.backend.session().animations() {
				candidate.synchronize_animation(bank)?;
			}
			workspace
				.backend
				.commit_world_region(prepared)
				.map_err(|error| error.to_string())?;
			let retired = std::mem::replace(&mut gpu.scene, candidate);
			gpu.revision = workspace.backend.session().scene_revision();
			gpu.change_cursor = workspace.backend.change_cursor();
			self.interaction.sync_game_status(workspace);
			self.interaction.clear_error();
			self.dirty = true;
			drop(shared);
			drop(retired);
			Ok(())
		});
		if let Some(gpu) = self.gpu.as_mut() {
			gpu.assets.prune_unused();
		}
		if let Err(error) = result {
			if let Some((_, stream)) = &mut self.world_streaming {
				if attaching {
					stream.reject(region);
				} else {
					stream.defer_detach(region);
				}
			}
			self.interaction
				.report(format!("Could not stream world region: {error}"));
		}
		Ok(())
	}
}
