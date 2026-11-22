//! Window-independent editor navigation and spatial tool math.
//!
//! The host owns capture and transactions. Nothing here mutates a scene, camera
//! document, selection, revision, history, file, or GPU resource.
use bridge::{
	editor::{TransformDto, TransformEditDto},
	viewport::{
		ViewportOrientation, ViewportPlane, ViewportProjection, ViewportSettingsDto, ViewportTool,
	},
};
use glam::{Mat4, Quat, Vec2, Vec3};
use runtime::Bounds3;

const EPSILON: f32 = 1.0e-6;
const MAX_DISTANCE: f32 = 1.0e7;
const MIN_DISTANCE: f32 = 0.001;

mod camera;
mod camera_frustum;
mod gizmo;
mod picking;

pub use camera::{project, EditorCamera};
pub use camera_frustum::camera_frustum;
pub use gizmo::{Gizmo, GizmoDrag, Handle, TransformTarget};
pub use picking::{
	adaptive_grid_spacing, pick_nearest, placement_point, plane_basis, PickTarget, Ray,
};

#[cfg(test)]
#[path = "../tests/unit/viewport_tools/mod.rs"]
mod tests;
