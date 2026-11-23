//! Stable wire identifiers for the editor's starter geometry.

use serde::{Deserialize, Serialize};

/// A built-in model that can be published as an ordinary project asset.
///
/// All geometry is Y-up. Solids have a centered pivot and the plane's pivot
/// lies on its surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrimitiveKind {
	/// A unit XZ plane facing +Y.
	Plane,
	/// A unit cube.
	Cube,
	/// A sphere with diameter one.
	Sphere,
	/// A Y-up capsule with diameter one and total height two.
	Capsule,
}

impl PrimitiveKind {
	pub const ALL: [Self; 4] = [Self::Plane, Self::Cube, Self::Sphere, Self::Capsule];

	/// The wire identifier and stable model filename stem.
	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Plane => "plane",
			Self::Cube => "cube",
			Self::Sphere => "sphere",
			Self::Capsule => "capsule",
		}
	}

	/// The default visible object and model name.
	pub const fn display_name(self) -> &'static str {
		match self {
			Self::Plane => "Plane",
			Self::Cube => "Cube",
			Self::Sphere => "Sphere",
			Self::Capsule => "Capsule",
		}
	}
}

#[cfg(test)]
mod tests {
	use super::PrimitiveKind;

	#[test]
	fn starter_primitive_ids_round_trip_and_are_distinct() {
		for (kind, expected) in PrimitiveKind::ALL
			.into_iter()
			.zip(["plane", "cube", "sphere", "capsule"])
		{
			assert_eq!(kind.as_str(), expected);
			let encoded = serde_json::to_value(kind).unwrap();
			assert_eq!(encoded, expected);
			assert_eq!(
				serde_json::from_value::<PrimitiveKind>(encoded).unwrap(),
				kind
			);
		}
	}

	#[test]
	fn starter_primitive_ids_reject_unknown_commands() {
		for invalid in ["\"triangle\"", "\"Cube\"", "0", "null"] {
			assert!(serde_json::from_str::<PrimitiveKind>(invalid).is_err());
		}
	}
}
