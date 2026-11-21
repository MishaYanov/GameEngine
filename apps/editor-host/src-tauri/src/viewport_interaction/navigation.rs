use super::*;

impl ViewportInteraction {
	pub(super) fn selected_bounds(&self, workspace: &EditorWorkspace) -> Option<Bounds3> {
		workspace
			.backend
			.session()
			.selection()
			.iter()
			.filter_map(|key| {
				self.target_indices
					.get(key.as_str())
					.map(|&index| self.targets[index].bounds)
			})
			.reduce(Bounds3::union)
	}

	pub(super) fn frame_selection(
		&mut self,
		workspace: &EditorWorkspace,
		extent: [u32; 2],
	) -> Result<(), String> {
		let bounds = self
			.selected_bounds(workspace)
			.or_else(|| {
				self.targets
					.iter()
					.map(|target| target.bounds)
					.reduce(Bounds3::union)
			})
			.ok_or("There is no renderable entity to frame")?;
		self.camera
			.frame(bounds, extent[0].max(1) as f32 / extent[1].max(1) as f32);
		Ok(())
	}

	pub(super) fn scene_shortcut(&self, code: &str, control: bool) -> Option<ViewportActionDto> {
		use ViewportActionDto::{FrameSelection, Orient, SetProjection, SetTool, Zoom};
		use ViewportOrientation::{Back, Bottom, Front, Left, Right, Top};
		use ViewportProjection::{Orthographic, Perspective};
		use ViewportTool::{Hand, Move, Rotate, Scale};
		Some(match (code, control) {
			("KeyQ", false) => SetTool { tool: Hand },
			("KeyW", false) => SetTool { tool: Move },
			("KeyE", false) => SetTool { tool: Rotate },
			("KeyR", false) => SetTool { tool: Scale },
			("KeyF" | "NumpadDecimal", false) => FrameSelection,
			("NumpadAdd", false) => Zoom { factor: 0.87 },
			("NumpadSubtract", false) => Zoom { factor: 1.0 / 0.87 },
			("Numpad1", false) => Orient { orientation: Front },
			("Numpad1", true) => Orient { orientation: Back },
			("Numpad3", false) => Orient { orientation: Right },
			("Numpad3", true) => Orient { orientation: Left },
			("Numpad7", false) => Orient { orientation: Top },
			("Numpad7", true) => Orient {
				orientation: Bottom,
			},
			("Numpad5", false) => SetProjection {
				projection: match self.settings.projection {
					Perspective => Orthographic,
					Orthographic => Perspective,
				},
			},
			_ => return None,
		})
	}

	pub(super) fn orbit_key(
		&mut self,
		code: &str,
		workspace: &mut EditorWorkspace,
	) -> Result<bool, String> {
		let step = std::f32::consts::PI / 12.0;
		let (yaw, pitch) = match code {
			"Numpad2" => (0.0, step),
			"Numpad4" => (step, 0.0),
			"Numpad6" => (-step, 0.0),
			"Numpad8" => (0.0, -step),
			"Numpad9" => (std::f32::consts::PI, -2.0 * self.camera.pitch),
			_ => return Ok(false),
		};
		self.cancel_transform(workspace)?;
		self.end_navigation();
		self.click = None;
		self.camera.orbit_angles(Vec2::new(yaw, pitch));
		self.changed();
		Ok(true)
	}

	pub(super) fn end_navigation(&mut self) {
		if self.navigation.take().is_some() {
			self.keys.clear();
			self.changed();
		}
	}

	pub fn is_flying(&self) -> bool {
		self.navigation
			.as_ref()
			.is_some_and(|nav| nav.mode == NavigationMode::Fly)
	}

	pub(super) fn persistent_fly(&self) -> bool {
		self.navigation
			.as_ref()
			.is_some_and(|nav| nav.walk_origin.is_some())
	}

	pub(super) fn restore_walk_camera(&mut self) {
		if let Some(original) = self
			.navigation
			.as_mut()
			.and_then(|nav| nav.walk_origin.take())
		{
			self.camera = original;
		}
	}

	pub fn look_from_fly_motion(&mut self, delta: [f64; 2]) {
		if self.is_flying() && delta != [0.0; 2] {
			// Device motion is relative, independent of the cursor reaching an edge.
			self.camera
				.look(Vec2::new(delta[0] as f32, delta[1] as f32) * 0.35);
			self.changed();
		}
	}

	pub(super) fn fly_movement(&self) -> Vec3 {
		let down = |key: &str| if self.keys.contains(key) { 1.0 } else { 0.0 };
		Vec3::new(
			down("KeyD") - down("KeyA"),
			down("KeyE") - down("KeyQ"),
			down("KeyW") - down("KeyS"),
		)
	}
	pub fn needs_camera_tick(&self) -> bool {
		self.is_flying() && self.fly_movement() != Vec3::ZERO
	}
	pub fn camera_tick(&mut self, seconds: f32) {
		if !self.needs_camera_tick() {
			return;
		}
		let movement = self.fly_movement();
		let speed = if self.keys.contains("ShiftLeft") || self.keys.contains("ShiftRight") {
			15.0
		} else {
			5.0
		};
		self.camera.fly(movement, seconds.min(0.05), speed);
		self.changed();
	}

	pub fn captured(&self) -> bool {
		self.navigation.is_some() || self.drag.is_some() || self.click.is_some()
	}
}
