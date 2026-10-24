use renderer::vulkan::{Light, LightingUniform};

#[test]
fn empty_authored_lights_remain_ambient_only_and_default_preserves_legacy_directional_light() {
	assert_eq!(LightingUniform::from_lights(&[]).unwrap().light_count(), 0);
	assert_eq!(LightingUniform::default().light_count(), 1);
	let light = Light::Directional {
		direction: [2.0, -4.0, -3.0],
		color: [1.0; 3],
		intensity: 0.75,
	};
	assert_eq!(
		LightingUniform::from_lights(&[light; 16])
			.unwrap()
			.light_count(),
		16
	);
	assert!(LightingUniform::from_lights(&[light; 17]).is_err());
	assert_eq!(std::mem::size_of::<LightingUniform>(), 16 + 16 * 48);
}

#[test]
fn light_inputs_reject_zero_direction_nonfinite_color_overflow_and_invalid_point_geometry() {
	for light in [
		Light::Directional {
			direction: [0.0; 3],
			color: [1.0; 3],
			intensity: 1.0,
		},
		Light::Directional {
			direction: [f32::NAN; 3],
			color: [1.0; 3],
			intensity: 1.0,
		},
		Light::Directional {
			direction: [0.0, -1.0, 0.0],
			color: [f32::NAN; 3],
			intensity: 1.0,
		},
		Light::Directional {
			direction: [0.0, -1.0, 0.0],
			color: [f32::MAX; 3],
			intensity: 2.0,
		},
		Light::Point {
			position: [0.0; 3],
			color: [1.0; 3],
			intensity: -1.0,
			range: 1.0,
		},
		Light::Point {
			position: [0.0; 3],
			color: [1.0; 3],
			intensity: 1.0,
			range: 0.0,
		},
		Light::Point {
			position: [f32::INFINITY; 3],
			color: [1.0; 3],
			intensity: 1.0,
			range: 1.0,
		},
	] {
		assert!(
			LightingUniform::from_lights(&[light]).is_err(),
			"accepted {light:?}"
		);
	}
}
