//! Stable camera-only projection for deterministic same-build replay comparison.
use crate::camera_input::CameraIntent;
use crate::runtime_lifecycle::RuntimeEntityHandle;
use crate::{Camera, EntityKey, game_camera::OrbitRuntimeState};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraReplayMode {
	Fixed,
	Follow,
	Orbit,
}

#[derive(Debug, Serialize)]
pub struct CameraPoseReplay {
	pub position: [f32; 3],
	pub yaw: f32,
	pub pitch: f32,
	pub vertical_fov: f32,
	pub near: f32,
	pub far: f32,
}

#[derive(Debug, Serialize)]
pub struct CameraTargetReplay {
	pub key: EntityKey,
	pub generation: u64,
	pub same_world: bool,
}

#[derive(Debug, Serialize)]
pub struct CameraHitReplay {
	pub blocker: EntityKey,
	pub fraction: f64,
}

#[derive(Debug, Serialize)]
pub struct OrbitReplay {
	pub target: CameraTargetReplay,
	pub target_lost: bool,
	pub yaw: f32,
	pub pitch: f32,
	pub user_distance: f32,
	pub effective_distance: f32,
	pub filtered_pivot: [f32; 3],
	pub previous_target_pivot: [f32; 3],
	pub last_allowed_distance: f32,
	pub obstruction: Option<CameraHitReplay>,
	pub pivot_settled: bool,
	pub arm_settled: bool,
	pub tick: u64,
}

#[derive(Debug, Serialize)]
pub struct PendingCameraReplay {
	pub yaw_delta: f64,
	pub pitch_delta: f64,
	pub log_distance_delta: f64,
}

#[derive(Debug, Serialize)]
pub struct CameraReplayProjection {
	pub schema_version: u32,
	pub mode: CameraReplayMode,
	pub pose: CameraPoseReplay,
	pub orbit: Option<OrbitReplay>,
	pub pending_intent: PendingCameraReplay,
}

fn finite32(value: f32, field: &str) -> Result<f32, String> {
	if !value.is_finite() {
		return Err(format!("Non-finite camera replay field: {field}"));
	}
	Ok(if value == 0.0 { 0.0 } else { value })
}

fn finite64(value: f64, field: &str) -> Result<f64, String> {
	if !value.is_finite() {
		return Err(format!("Non-finite camera replay field: {field}"));
	}
	Ok(if value == 0.0 { 0.0 } else { value })
}

fn finite3(value: [f32; 3], field: &str) -> Result<[f32; 3], String> {
	Ok([
		finite32(value[0], field)?,
		finite32(value[1], field)?,
		finite32(value[2], field)?,
	])
}

fn replay_target(target: &RuntimeEntityHandle, current_world_id: &str) -> CameraTargetReplay {
	CameraTargetReplay {
		key: target.key.clone(),
		generation: target.generation,
		same_world: target.world_id == current_world_id,
	}
}

fn replay_pose(camera: &Camera) -> Result<CameraPoseReplay, String> {
	Ok(CameraPoseReplay {
		position: finite3(camera.position, "position")?,
		yaw: finite32(camera.yaw, "yaw")?,
		pitch: finite32(camera.pitch, "pitch")?,
		vertical_fov: finite32(camera.vertical_fov, "vertical_fov")?,
		near: finite32(camera.near, "near")?,
		far: finite32(camera.far, "far")?,
	})
}

fn replay_pending(intent: CameraIntent) -> Result<PendingCameraReplay, String> {
	// This is a latch aggregate, not one external sample. Multiple valid
	// samples can reach TAU even though each sample is limited to PI.
	intent.validate_step()?;
	Ok(PendingCameraReplay {
		yaw_delta: finite64(intent.yaw_delta, "pending.yaw")?,
		pitch_delta: finite64(intent.pitch_delta, "pending.pitch")?,
		log_distance_delta: finite64(intent.log_distance_delta, "pending.zoom")?,
	})
}

fn replay_orbit(state: &OrbitRuntimeState, current_world_id: &str) -> Result<OrbitReplay, String> {
	let obstruction = state
		.obstruction
		.as_ref()
		.map(|hit| -> Result<CameraHitReplay, String> {
			let fraction = finite64(hit.fraction, "obstruction.fraction")?;
			if !(0.0..=1.0).contains(&fraction) {
				return Err("Camera replay hit fraction is outside [0, 1]".into());
			}
			Ok(CameraHitReplay {
				blocker: hit.blocker.clone(),
				fraction,
			})
		})
		.transpose()?;

	Ok(OrbitReplay {
		target: replay_target(&state.target, current_world_id),
		target_lost: state.target_lost,
		yaw: finite32(state.yaw, "orbit.yaw")?,
		pitch: finite32(state.pitch, "orbit.pitch")?,
		user_distance: finite32(state.user_distance, "user_distance")?,
		effective_distance: finite32(state.effective_distance, "effective_distance")?,
		filtered_pivot: finite3(state.filtered_pivot, "filtered_pivot")?,
		previous_target_pivot: finite3(state.previous_target_pivot, "previous_target_pivot")?,
		last_allowed_distance: finite32(state.last_allowed_distance, "last_allowed_distance")?,
		obstruction,
		pivot_settled: state.pivot_settled,
		arm_settled: state.arm_settled,
		tick: state.tick,
	})
}

impl CameraReplayProjection {
	pub(crate) fn from_committed(
		camera: &Camera,
		orbit: Option<&OrbitRuntimeState>,
		follow: bool,
		intent: CameraIntent,
		current_world_id: &str,
	) -> Result<Self, String> {
		let mode = match (orbit, follow) {
			(Some(_), _) => CameraReplayMode::Orbit,
			(None, true) => CameraReplayMode::Follow,
			(None, false) => CameraReplayMode::Fixed,
		};
		Ok(Self {
			schema_version: 1,
			mode,
			pose: replay_pose(camera)?,
			orbit: orbit
				.map(|state| replay_orbit(state, current_world_id))
				.transpose()?,
			pending_intent: replay_pending(intent)?,
		})
	}
}

#[cfg(test)]
#[path = "../tests/unit/camera_replay.rs"]
mod tests;
