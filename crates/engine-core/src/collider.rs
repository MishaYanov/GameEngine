//! Authored collider geometry and interaction settings shared by all collision paths.
use serde::{
	Deserialize, Deserializer, Serialize,
	de::{MapAccess, Visitor, value::MapAccessDeserializer},
};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ColliderInteraction {
	/// Participates in overlap detection and never blocks kinematic movement.
	pub sensor: bool,
	/// Membership bits. Zero disables detection with every collider.
	pub layers: u32,
	/// Accepted membership bits. All bits accepts every layer, subject to the other mask.
	pub mask: u32,
}

impl<'de> Deserialize<'de> for ColliderInteraction {
	fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		struct InteractionVisitor;
		impl<'de> Visitor<'de> for InteractionVisitor {
			type Value = ColliderInteraction;

			fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
				formatter.write_str("a collider interaction object")
			}

			fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<Self::Value, M::Error> {
				// Reuse derived field/default validation while requiring object form.
				#[derive(Deserialize)]
				#[serde(remote = "ColliderInteraction", default, deny_unknown_fields)]
				struct InteractionFields {
					sensor: bool,
					layers: u32,
					mask: u32,
				}
				impl Default for InteractionFields {
					fn default() -> Self {
						let defaults = ColliderInteraction::default();
						Self {
							sensor: defaults.sensor,
							layers: defaults.layers,
							mask: defaults.mask,
						}
					}
				}
				InteractionFields::deserialize(MapAccessDeserializer::new(map))
			}
		}
		deserializer.deserialize_map(InteractionVisitor)
	}
}

impl Default for ColliderInteraction {
	fn default() -> Self {
		Self {
			sensor: false,
			layers: 1,
			mask: u32::MAX,
		}
	}
}

impl ColliderInteraction {
	pub fn is_default(&self) -> bool {
		*self == Self::default()
	}

	/// Both masks must accept the other collider's layers. A zero mask or zero
	/// layers on either participant disables the pair. Sensor state does not filter overlaps.
	pub fn detects(self, other: Self) -> bool {
		(self.layers & other.mask) != 0 && (other.layers & self.mask) != 0
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ColliderData {
	Box {
		half_extents: [f32; 3],
		offset: [f32; 3],
		#[serde(default, skip_serializing_if = "ColliderInteraction::is_default")]
		interaction: ColliderInteraction,
	},
	Sphere {
		radius: f32,
		offset: [f32; 3],
		#[serde(default, skip_serializing_if = "ColliderInteraction::is_default")]
		interaction: ColliderInteraction,
	},
}

impl ColliderData {
	pub fn interaction(&self) -> ColliderInteraction {
		match self {
			Self::Box { interaction, .. } | Self::Sphere { interaction, .. } => *interaction,
		}
	}

	pub fn validate(&self) -> Result<(), String> {
		let (offset, valid_size) = match self {
			Self::Box {
				half_extents,
				offset,
				..
			} => (
				offset,
				half_extents.iter().all(|v| v.is_finite() && *v > 0.0),
			),
			Self::Sphere { radius, offset, .. } => (offset, radius.is_finite() && *radius > 0.0),
		};
		if !valid_size || !offset.iter().all(|v| v.is_finite()) {
			return Err("Collider extents/radius must be positive and all values finite".into());
		}
		Ok(())
	}
}
