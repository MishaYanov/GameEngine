use crate::Transform;
use crate::assets::Model;
use std::sync::Arc;

#[derive(Clone)]
pub struct ModelInstance {
	model: Arc<Model>,
	transform: Transform,
	animation: Option<Arc<NativeAnimationPose>>,
	base_animation: Option<Arc<NativeAnimationPose>>,
	authored_animation: Option<Arc<NativeAnimationPose>>,
}

impl ModelInstance {
	pub fn new(model: Arc<Model>, transform: Transform) -> Self {
		Self {
			model,
			transform,
			animation: None,
			base_animation: None,
			authored_animation: None,
		}
	}

	pub fn model(&self) -> &Model {
		self.model.as_ref()
	}

	pub fn transform(&self) -> &Transform {
		&self.transform
	}

	pub fn transform_mut(&mut self) -> &mut Transform {
		&mut self.transform
	}
}

#[derive(Clone)]
struct NativeAnimationPose {
	palette: Vec<[f32; 16]>,
	mesh_matrix: glam::Mat4,
	bounds: crate::Bounds3,
}
impl ModelInstance {
	pub fn set_animation_pose(
		&mut self,
		pose: &engine_core::animation::AnimationPose,
		base: bool,
	) -> Result<(), String> {
		let asset = self
			.model
			.animation_asset()
			.ok_or("Native model has no skin influences")?;
		if asset.source_sha256 != pose.asset.source_sha256
			|| asset.skin.source_mesh_node != pose.asset.skin.source_mesh_node
			|| asset.skin.source_skin != pose.asset.skin.source_skin
		{
			return Err("Animation pose belongs to another model snapshot".into());
		}
		let render = Arc::new(NativeAnimationPose {
			palette: pose.palette.iter().map(|m| m.to_cols_array()).collect(),
			mesh_matrix: pose.mesh_matrix,
			bounds: engine_core::animation::animated_bounds(
				&pose.asset,
				&pose.globals,
				glam::Mat4::IDENTITY,
			)?,
		});
		if base {
			if self.authored_animation.is_none() {
				self.authored_animation = Some(render.clone());
			}
			self.base_animation = Some(render.clone());
		}
		self.animation = Some(render);
		Ok(())
	}
	pub fn restore_authored_animation(&mut self) {
		self.base_animation.clone_from(&self.authored_animation);
		self.animation.clone_from(&self.authored_animation);
	}
	pub fn reset_animation_override(&mut self) {
		self.animation.clone_from(&self.base_animation);
	}
	pub fn animation_palette(&self) -> Option<&[[f32; 16]]> {
		self.animation.as_ref().map(|p| p.palette.as_slice())
	}
	pub fn animation_mesh_matrix(&self) -> glam::Mat4 {
		self.animation
			.as_ref()
			.map_or(glam::Mat4::IDENTITY, |p| p.mesh_matrix)
	}
	pub fn animation_bounds(&self) -> Option<crate::Bounds3> {
		self.animation.as_ref().map(|p| p.bounds)
	}
}
