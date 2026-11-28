//! World-axis translation with a frozen parent transform and draft binding.
use crate::viewport_tools::{EditorCamera, Gizmo, GizmoDrag, TransformTarget};
use bridge::viewport::{ViewportProjection, ViewportSettingsDto, ViewportTool};
use glam::{Mat4, Quat, Vec3};
use renderer::EditorLine;
use runtime::{scene::Scene, EntityKey};

pub(super) struct PropTranslationDrag {
	pub key: EntityKey,
	pub revision: u64,
	pub extent: [u32; 2],
	baseline: [f32; 3],
	parent_inverse: Mat4,
	geometry: GizmoDrag,
	position: [f32; 3],
}

impl PropTranslationDrag {
	pub fn begin(
		scene: &Scene,
		camera: &EditorCamera,
		extent: [u32; 2],
		dpi: f64,
		key: EntityKey,
		revision: u64,
		point: [f64; 2],
	) -> Option<Self> {
		let id = scene.entity_by_key(&key)?;
		let world = scene.world_matrix(id).ok()?;
		let gizmo = move_gizmo(world, camera, extent, dpi);
		let handle = gizmo.hit_test(
			camera,
			extent,
			ViewportProjection::Perspective,
			ViewportTool::Move,
			point,
			(8.0 * dpi) as f32,
		)?;
		let parent_inverse = match scene.parent(id) {
			Some(parent) => scene
				.world_matrix(scene.entity_by_key(parent)?)
				.ok()?
				.inverse(),
			None => Mat4::IDENTITY,
		};
		if !parent_inverse.is_finite() {
			return None;
		}
		let geometry = GizmoDrag::begin(
			camera,
			extent,
			ViewportProjection::Perspective,
			point,
			gizmo,
			ViewportTool::Move,
			handle,
			&ViewportSettingsDto {
				snap: false,
				..ViewportSettingsDto::default()
			},
			vec![TransformTarget {
				key: key.as_str().into(),
				world,
			}],
		)
		.ok()?;
		let baseline = scene.get(id)?.transform().position;
		Some(Self {
			key,
			revision,
			extent,
			baseline,
			parent_inverse,
			geometry,
			position: baseline,
		})
	}

	pub fn update(&mut self, point: [f64; 2]) -> Result<(), String> {
		let world_delta = self.geometry.translation_delta(point)?;
		let local =
			Vec3::from_array(self.baseline) + self.parent_inverse.transform_vector3(world_delta);
		if !local.is_finite() {
			return Err("Prop position exceeds the supported range".into());
		}
		self.position = local.to_array();
		Ok(())
	}

	pub fn position(&self) -> [f32; 3] {
		self.position
	}

	pub fn moved(&self) -> bool {
		self.position != self.baseline
	}
}

pub(super) fn move_handles(
	scene: &Scene,
	camera: &EditorCamera,
	extent: [u32; 2],
	dpi: f64,
	key: Option<&EntityKey>,
) -> Vec<EditorLine> {
	let Some(world) = key
		.and_then(|key| scene.entity_by_key(key))
		.and_then(|id| scene.world_matrix(id).ok())
	else {
		return Vec::new();
	};
	const COLORS: [[f32; 4]; 3] = [
		[0.95, 0.24, 0.30, 1.0],
		[0.28, 0.80, 0.46, 1.0],
		[0.35, 0.58, 1.0, 1.0],
	];
	move_gizmo(world, camera, extent, dpi)
		.visible_segments(
			camera,
			extent,
			ViewportProjection::Perspective,
			ViewportTool::Move,
		)
		.into_iter()
		.map(|segment| {
			EditorLine::new(
				segment.a.to_array(),
				segment.b.to_array(),
				COLORS[segment.axis],
			)
		})
		.collect()
}

fn move_gizmo(world: Mat4, camera: &EditorCamera, extent: [u32; 2], dpi: f64) -> Gizmo {
	let pivot = world.w_axis.truncate();
	let scale = camera.gizmo_size(
		pivot,
		extent,
		ViewportProjection::Perspective,
		(72.0 * dpi) as f32,
	);
	Gizmo::new(pivot, Quat::IDENTITY, scale)
}
