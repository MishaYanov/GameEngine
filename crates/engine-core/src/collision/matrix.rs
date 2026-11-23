use crate::{
	collider::ColliderInteraction,
	project::{COLLISION_LAYER_COUNT, PhysicsLayers},
};

/// Compiled, reciprocal rules for collision-layer pairs within one project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InteractionMatrix {
	allowed: [u32; COLLISION_LAYER_COUNT as usize],
}

impl InteractionMatrix {
	pub const ALLOW_ALL: Self = Self {
		allowed: [u32::MAX; COLLISION_LAYER_COUNT as usize],
	};

	/// Compile the authored policy once. An absent policy retains legacy behavior.
	pub fn compile(layers: Option<&PhysicsLayers>) -> Result<Self, String> {
		let mut matrix = Self::ALLOW_ALL;
		if let Some(layers) = layers {
			layers.validate()?;
			for &[left, right] in &layers.ignored_pairs {
				matrix.allowed[usize::from(left)] &= !(1u32 << right);
				matrix.allowed[usize::from(right)] &= !(1u32 << left);
			}
		}
		Ok(matrix)
	}

	/// Both raw masks must accept the same layer pair that the project permits.
	pub fn allows_pair(&self, left: ColliderInteraction, right: ColliderInteraction) -> bool {
		let mut accepted_left = left.layers & right.mask;
		let accepted_right = right.layers & left.mask;
		if accepted_right == 0 {
			return false;
		}
		while accepted_left != 0 {
			let bit = accepted_left.trailing_zeros() as usize;
			if self.allowed[bit] & accepted_right != 0 {
				return true;
			}
			accepted_left &= accepted_left - 1;
		}
		false
	}

	pub(super) fn allows_blocking(
		&self,
		left: ColliderInteraction,
		right: ColliderInteraction,
	) -> bool {
		!left.sensor && !right.sensor && self.allows_pair(left, right)
	}
}
