use super::*;
use gameplay_catalog::cinder_oath::Phase;
use renderer::ui::{ScreenOverlay, ScreenOverlayLayout};

use super::player_controls::input_labels::compact_key_name;

impl GameApp {
	pub(super) fn render(&mut self) -> Result<renderer::DrawStatus, String> {
		let result = self.render_frame();
		let outcome = match result {
			Ok(renderer::DrawStatus::Rendered) => Some(SceneLoadOutcome::Presented),
			Err(_) => Some(SceneLoadOutcome::Failed),
			_ => None,
		};
		if let Some(outcome) = outcome
			&& let Some(timing) = self.scene_load_timing.take()
		{
			timing.finish(outcome);
		}
		if matches!(result, Ok(renderer::DrawStatus::Rendered))
			&& let (Some(prefetch), Some(current), Some(project)) = (
			&mut self.scene_prefetch,
			self.editor.play_scene_id(),
			self.project.as_ref(),
		) {
			prefetch.prime(current, project);
		}
		result
	}

	fn render_frame(&mut self) -> Result<renderer::DrawStatus, String> {
		if let Some(camera) = self.editor.play_rig_camera() {
			self.camera = camera;
		}
		let dirty = !self.standalone_runtime && self.is_dirty();
		let cinder_phase = if self.standalone_runtime && !self.player_controls.is_open() {
			self.cinder_phase()
		} else {
			None
		};
		let cinder_duel = cinder_phase == Some(Phase::Duel);
		let mouse_captured = self.mouse_captured;
		let Some(renderer) = self.renderer.as_mut() else {
			return Ok(renderer::DrawStatus::Suspended);
		};
		let Some([width, height]) = renderer.prepare()? else {
			return Ok(renderer::DrawStatus::Suspended);
		};
		let game_rect = if self.standalone_runtime || self.editor.is_playing() {
			Some(
				GameContentRect::fit(width, height)
					.ok_or("Game viewport extent is out of range")?,
			)
		} else {
			None
		};
		let content_extent = game_rect.map_or([width, height], GameContentRect::extent);
		let scale = self
			.window
			.as_ref()
			.map_or(1.0, |window| window.scale_factor());
		let display = self.player_controls.display();
		let style = display.overlay_style();
		let mut hud_rows = Vec::new();
		if self.standalone_runtime {
			let capacity = ScreenOverlay::capacity_with_style(content_extent, scale, style);
			if capacity.rows > 0 {
				hud_rows = self
					.player_controls
					.rows_with_feedback(capacity.rows, capacity.columns, &self.feedback_audio)
					.unwrap_or_else(|| {
						let mut rows = gameplay_hud_rows(self.editor.play_status().as_ref());
						if cinder_phase.is_some() {
							rows.retain(|row| !row.starts_with("Score "));
						}
						if cinder_phase == Some(Phase::Intro) {
							rows.truncate(capacity.rows);
							return rows;
						}
						if cinder_duel {
							rows.push(if mouse_captured {
								"WASD Move  /  Mouse Aim"
							} else {
								"Click to aim  /  WASD Move"
							}.into());
							rows.push("LMB or F Strike".into());
						}
						if cinder_phase == Some(Phase::Defeat) {
							rows.push("F10 to restart".into());
						}
						let mut required = usize::from(!rows.is_empty());
						if let Some(prompt) = rows
							.iter()
							.position(|row| *row == "Use Interact to open the route gate")
						{
							rows[prompt] = interact_hint_for_columns(
								self.input_profile_override
									.as_ref()
									.or_else(|| self.project.as_ref()?.input_profile.as_ref()),
								self.interaction_action.as_deref(),
								self.player_controls.controller(),
								self.player_controls.prompt_device(),
								capacity.columns,
							);
							required = (prompt + 1).min(capacity.rows);
							if prompt >= capacity.rows {
								rows.swap(capacity.rows - 1, prompt);
							}
						}
						rows.truncate(capacity.rows);
						if display.cue_captions {
							insert_captions(
								&mut rows,
								required,
								capacity.rows,
								self.feedback_captions.rows(Instant::now()),
							);
						}
						if rows.len() < capacity.rows
							&& let Some(trace) = &self.play_trace
						{
							rows.push(if trace.is_full() {
								"Playtrace full. Quit to save".into()
							} else {
								"Recording playtrace".into()
							});
						}
						if rows.len() < capacity.rows {
							rows.push(if cfg!(windows) {
								"Esc / Menu: Controls".into()
							} else {
								"Esc: Controls".into()
							});
						}
						rows
					});
			}
		}
		let camera_uniform = CameraUniform::perspective(
			self.camera.position,
			self.camera.target(),
			[0.0, 1.0, 0.0],
			self.camera.vertical_fov,
			game_rect.map_or(width as f32 / height as f32, GameContentRect::aspect),
			self.camera.near,
			self.camera.far,
		);

		let frustum = if self.tools.culling {
			Some(runtime::visibility::Frustum::from_view_projection(
				glam::Mat4::from_cols_array(&camera_uniform.view_projection),
			)?)
		} else {
			None
		};
		let (mut objects, mut stats) = self.scene.try_render_visible(frustum.as_ref())?;
		if !self.standalone_runtime
			&& self.authored.is_none()
			&& let Some(object) = self
				.selection_overlay
				.as_ref()
				.and_then(|overlay| overlay.object(&self.scene, &self.editor))
		{
			stats.draw_calls += 1;
			stats.triangles += object.mesh().index_count() as usize / 3;
			objects.push(object);
		}

		if !self.standalone_runtime {
			if self.authored.is_none()
				&& let Some(transform) = self
					.editor
					.selected_entity()
					.map(|id| runtime::gizmo::GizmoPose::from_scene(&self.scene, id))
					.transpose()?
				&& let Some(gizmo) = self.tools.gizmo.as_mut()
			{
				gizmo.prepare(
					renderer,
					&self.camera,
					content_extent,
					transform,
					self.tools.panel.mode,
					self.tools.drag.as_ref().map(|drag| drag.axis),
				)?;
				let object = gizmo.object();
				stats.draw_calls += 1;
				stats.triangles += object.mesh().index_count() as usize / 3;
				objects.push(object);
			}
			// The panel is one batched draw. Include its previous geometry in
			// displayed totals; the current text has the same quad count.
			if let Some(panel) = self.tools.panel.object() {
				stats.draw_calls += 1;
				stats.triangles += panel.mesh().index_count() as usize / 3;
			}
			self.tools.panel.prepare(
				renderer,
				content_extent,
				scale,
				PanelView {
					scene: &self.scene,
					editor: &self.editor,
					models: &self.tools.models,
					playing: self.authored.is_some(),
					dirty,
					stats,
					frame_ms: self.tools.metrics.frame_ms,
					cpu_ms: self.tools.metrics.cpu_ms,
					culling: self.tools.culling,
				},
			)?;
			if let Some(panel) = self.tools.panel.object() {
				objects.push(panel);
			}
		}
		if self.standalone_runtime && (!hud_rows.is_empty() || cinder_duel) {
			if self.game_hud.is_none() {
				self.game_hud = Some(ScreenOverlay::new(renderer)?);
			}
			let hud = self.game_hud.as_mut().expect("initialized HUD");
			let panel = match cinder_phase {
				Some(phase) => ScreenOverlayLayout::Compact {
					bottom: phase == Phase::Intro,
					aim: cinder_duel,
				},
				None => ScreenOverlayLayout::Standard,
			};
			hud.prepare_with_layout(renderer, content_extent, scale, &hud_rows, style, panel)?;
			if let Some(object) = hud.object() {
				objects.push(object);
			}
		}
		self.tools.stats = stats;

		renderer.set_lights(&self.scene.renderer_lights())?;
		match game_rect {
			Some(rect) => renderer.draw_in_rect(&camera_uniform, &objects, rect.pixels()),
			None => renderer.draw(&camera_uniform, &objects),
		}
	}
}

#[cfg(test)]
pub(super) fn interact_hint(
	profile: Option<&InputProfile>,
	action: Option<&str>,
	bindings: runtime::input::ControllerBindings,
	device: PromptDevice,
) -> String {
	interact_hint_for_columns(profile, action, bindings, device, usize::MAX)
}

fn interact_hint_for_columns(
	profile: Option<&InputProfile>,
	action: Option<&str>,
	bindings: runtime::input::ControllerBindings,
	device: PromptDevice,
	columns: usize,
) -> String {
	let binding =
		action.and_then(|action| profile?.actions.iter().find(|binding| binding.id == action));
	let Some(binding) = binding else {
		let full = "Interact is unbound in the active profile";
		return if full.len() <= columns {
			full
		} else {
			"Interact unbound"
		}
			.into();
	};
	let (key, source) = if device == PromptDevice::Controller && binding.id == "interact" {
		(format!("{:?}", bindings.interact).into(), "Controller")
	} else {
		(compact_key_name(binding.key), "Keyboard")
	};
	let full = format!("Press {key} ({source}) to open the route gate");
	if full.len() <= columns {
		full
	} else {
		let compact = format!("{key}: Open gate");
		if compact.len() <= columns {
			compact
		} else {
			key.into_owned()
		}
	}
}

fn insert_captions(
	rows: &mut Vec<String>,
	mut after_required: usize,
	capacity: usize,
	captions: impl Iterator<Item=&'static str>,
) {
	for caption in captions {
		if after_required >= capacity {
			break;
		}
		if rows.iter().any(|row| {
			let status = row
				.rsplit_once(": ")
				.map_or(row.as_str(), |(_, suffix)| suffix);
			status.eq_ignore_ascii_case(caption)
		}) {
			continue;
		}
		rows.insert(after_required, caption.into());
		rows.truncate(capacity);
		after_required += 1;
	}
}

pub(super) fn gameplay_hud_rows(
	status: Option<&runtime::editor::command::PlayStatusDto>,
) -> Vec<String> {
	let Some(status) = status else {
		return Vec::new();
	};
	let mut rows = Vec::new();
	if let Some(gameplay) = &status.gameplay {
		rows.push(gameplay.objective.clone());
		if let Some(detail) = &gameplay.detail {
			rows.push(detail.clone());
		}
		if let Some(score) = gameplay.score {
			rows.push(format!("Score {score}"));
		}
		if gameplay.completed {
			rows.push("F10 to restart".into());
		}
	}
	if status.paused {
		rows.push("Paused".into());
	}
	if let Some(scene) = &status.pending_scene_id {
		rows.push(format!("Scene {} pending. F8 to retry", scene.as_str()));
	}
	rows
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn captions_keep_objective_and_prompt_before_optional_rows() {
		let mut rows = vec![
			"Find gate".to_owned(),
			"E: Open gate".to_owned(),
			"Score 1".to_owned(),
		];
		insert_captions(
			&mut rows,
			2,
			3,
			["Key collected", "Gate opened"].into_iter(),
		);
		assert_eq!(rows, ["Find gate", "E: Open gate", "Key collected"]);

		let mut rows = vec!["Reach exit".to_owned(), "Garden: Key collected".to_owned()];
		insert_captions(&mut rows, 1, 3, ["Key collected"].into_iter());
		assert_eq!(rows, ["Reach exit", "Garden: Key collected"]);
	}
}
