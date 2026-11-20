//! Selected collision outlines, independent of model bounds and renderer state.
use std::collections::HashMap;

use glam::{Mat4, Vec3};
use renderer::{EditorLine, EditorVisuals, MAX_EDITOR_LINES};
use runtime::{
	collision::{debug_shape, DebugShape},
	EntityKey, SceneData,
};

use crate::editor::EditorWorkspace;

const MAX_SELECTED: usize = 64;
const MAX_LINES: usize = MAX_EDITOR_LINES / 8;
const MAX_PARENT_VISITS: usize = 8192;
const RING_SEGMENTS: usize = 32;
const COLOR: [f32; 4] = [0.2, 0.95, 0.95, 1.0];
const SENSOR_COLOR: [f32; 4] = [1.0, 0.55, 0.95, 1.0];

#[derive(Default)]
pub(super) struct ColliderVisuals {
	key: Option<(u64, u64)>,
	selection: Vec<EntityKey>,
	indices: HashMap<EntityKey, usize>,
	lines: Vec<EditorLine>,
	error: Option<String>,
	#[cfg(test)]
	pub(super) rebuilds: usize,
	#[cfg(test)]
	pub(super) index_builds: usize,
}

impl ColliderVisuals {
	pub fn error(&self) -> Option<&str> {
		self.error.as_deref()
	}

	pub fn sync(&mut self, workspace: &EditorWorkspace, enabled: bool) -> bool {
		if !enabled {
			self.key = None;
			self.lines.clear();
			return self.error.take().is_some();
		}
		let session = workspace.backend.session();
		let key = (workspace.generation, session.scene_revision());
		if self.key == Some(key) && self.selection == session.selection() {
			return false;
		}
		let document = session.render_document();
		if self.key.is_none_or(|previous| previous.0 != key.0) {
			self.reindex(&document);
		}
		self.key = Some(key);
		self.selection = session.selection().to_vec();
		self.lines.clear();
		let previous_error = self.error.take();
		#[cfg(test)]
		{
			self.rebuilds += 1;
		}
		let mut matrices = HashMap::new();
		let mut visits = MAX_PARENT_VISITS;
		let mut omitted = 0;
		let mut colliders = 0;
		for selected in 0..self.selection.len() {
			let entity = self.selection[selected].clone();
			let shape = (|| {
				let index = self.index(&document, &entity)?;
				let Some(collider) = document.entities[index].collider.as_ref() else {
					return Ok(None);
				};
				if colliders == MAX_SELECTED {
					omitted += 1;
					return Ok(None);
				}
				colliders += 1;
				let matrix = self.world_matrix(&document, &entity, &mut matrices, &mut visits)?;
				debug_shape(&entity, collider, matrix)
					.map(|shape| Some((shape, collider.interaction().sensor)))
			})();
			match shape {
				Ok(Some((shape, sensor))) => {
					let count = match (&shape, sensor) {
						(DebugShape::Box { .. }, false) => 12,
						(DebugShape::Box { .. }, true) => 48,
						(DebugShape::Sphere { .. }, false) => RING_SEGMENTS * 3,
						(DebugShape::Sphere { .. }, true) => RING_SEGMENTS * 3 / 2,
					};
					if self.lines.len() + count <= MAX_LINES {
						append_shape(&mut self.lines, shape, sensor);
					} else {
						omitted += 1;
					}
				}
				Ok(None) => {}
				Err(error) => {
					if self.error.is_none() {
						self.error = Some(format!(
							"Collider outline for {:?} could not be drawn: {error}",
							entity.as_str()
						));
					}
				}
			}
		}
		if omitted > 0 && self.error.is_none() {
			self.error = Some(format!("Collider outline limit reached. Select fewer entities to see {omitted} omitted outlines."));
		}
		previous_error != self.error
	}

	pub fn append(&self, visuals: &mut EditorVisuals, dpi: f64) {
		let available = MAX_EDITOR_LINES.saturating_sub(visuals.lines.len());
		visuals
			.lines
			.extend(self.lines.iter().take(available).copied().map(|mut line| {
				line.width = (1.8 * dpi as f32).clamp(0.1, 32.0);
				line
			}));
	}

	fn reindex(&mut self, document: &SceneData) {
		self.indices = document
			.entities
			.iter()
			.enumerate()
			.map(|(index, entity)| (entity.key.clone(), index))
			.collect();
		#[cfg(test)]
		{
			self.index_builds += 1;
		}
	}

	fn index(&mut self, document: &SceneData, entity: &EntityKey) -> Result<usize, String> {
		if let Some(&index) = self.indices.get(entity) {
			if document
				.entities
				.get(index)
				.is_some_and(|candidate| &candidate.key == entity)
			{
				return Ok(index);
			}
		}
		self.reindex(document);
		self.indices
			.get(entity)
			.copied()
			.ok_or_else(|| "Selected entity or parent no longer exists".into())
	}

	fn world_matrix(
		&mut self,
		document: &SceneData,
		entity: &EntityKey,
		matrices: &mut HashMap<usize, Mat4>,
		visits: &mut usize,
	) -> Result<Mat4, String> {
		let mut current = entity.clone();
		let mut chain = Vec::new();
		let mut matrix = loop {
			*visits = visits
				.checked_sub(1)
				.ok_or("Collider outline hierarchy limit reached")?;
			let index = self.index(document, &current)?;
			if let Some(matrix) = matrices.get(&index) {
				break *matrix;
			}
			let entity = &document.entities[index];
			chain.push((index, entity.transform.matrix()));
			let Some(parent) = &entity.parent else {
				break Mat4::IDENTITY;
			};
			current = parent.clone();
		};
		for (index, local) in chain.into_iter().rev() {
			matrix *= local;
			if !matrix.is_finite() {
				return Err("Collider world transform exceeds finite limits".into());
			}
			matrices.insert(index, matrix);
		}
		Ok(matrix)
	}
}

fn append_shape(lines: &mut Vec<EditorLine>, shape: DebugShape, sensor: bool) {
	let color = if sensor { SENSOR_COLOR } else { COLOR };
	match shape {
		DebugShape::Box { min, max, .. } => {
			let corners: [[f32; 3]; 8] = std::array::from_fn(|i| {
				std::array::from_fn(|axis| {
					if i & (1 << axis) == 0 {
						min[axis]
					} else {
						max[axis]
					}
				})
			});
			for i in 0..8 {
				for axis in [1, 2, 4] {
					let other = i ^ axis;
					if i < other {
						if sensor {
							// Gaps distinguish sensors without relying only on color.
							let start = Vec3::from_array(corners[i]);
							let end = Vec3::from_array(corners[other]);
							let point = |part: u32| {
								let t = part as f32 / 7.0;
								(start * (1.0 - t) + end * t).to_array()
							};
							for dash in 0..4 {
								lines.push(EditorLine::new(
									point(dash * 2),
									point(dash * 2 + 1),
									color,
								));
							}
						} else {
							lines.push(EditorLine::new(corners[i], corners[other], color));
						}
					}
				}
			}
		}
		DebugShape::Sphere { center, radius, .. } => {
			let center = Vec3::from_array(center);
			for (first, second) in [(Vec3::X, Vec3::Y), (Vec3::X, Vec3::Z), (Vec3::Y, Vec3::Z)] {
				for index in 0..RING_SEGMENTS {
					if sensor && index % 2 != 0 {
						continue;
					}
					let point = |step: usize| {
						let angle = std::f32::consts::TAU * step as f32 / RING_SEGMENTS as f32;
						(center + (first * angle.cos() + second * angle.sin()) * radius).to_array()
					};
					lines.push(EditorLine::new(point(index), point(index + 1), color));
				}
			}
		}
	}
}
