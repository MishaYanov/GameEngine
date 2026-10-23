use super::*;

#[test]
fn validation_rejects_every_nonfinite_component_with_its_field_name() {
	for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
		for index in 0..9 {
			let mut transform = Transform::default();
			let (name, vector) = match index / 3 {
				0 => ("position", &mut transform.position),
				1 => ("rotation", &mut transform.rotation),
				_ => ("scale", &mut transform.scale),
			};
			vector[index % 3] = invalid;
			assert!(transform.validate().unwrap_err().contains(name));
		}
	}
}

#[test]
fn finite_negative_and_zero_scale_are_valid() {
	assert!(Transform::default().validate().is_ok());
	assert!(
		Transform {
			position: [-1.0, 0.0, f32::MAX],
			rotation: [-3.0, 0.0, 3.0],
			scale: [-1.0, 0.0, 2.0]
		}
			.validate()
			.is_ok()
	);
}
