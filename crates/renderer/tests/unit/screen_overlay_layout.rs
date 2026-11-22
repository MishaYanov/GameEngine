use super::*;

#[test]
fn overlay_rejects_invalid_geometry_and_keeps_capacity_bounded() {
	let standard = ScreenOverlayStyle::default();
	for dpi in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MIN_POSITIVE] {
		assert_eq!(
			ScreenOverlay::capacity_with_style([640, 360], dpi, standard),
			ScreenOverlayCapacity::default(),
		);
	}
	assert_eq!(ScreenOverlay::row_capacity([0, 360], 1.0), 0);
	assert_eq!(ScreenOverlay::row_capacity([640, 0], 1.0), 0);
	assert_eq!(ScreenOverlay::row_capacity([640, 1080], 1.0), 8);
	let invalid_style = ScreenOverlayStyle {
		text_scale: f32::NAN,
		..standard
	};
	assert_eq!(
		ScreenOverlay::capacity_with_style([640, 360], 1.0, invalid_style),
		ScreenOverlayCapacity::default(),
	);
	let enlarged = ScreenOverlay::capacity_with_style(
		[640, 360],
		1.5,
		ScreenOverlayStyle {
			text_scale: 1.5,
			high_contrast: true,
		},
	);
	assert_eq!(
		enlarged,
		ScreenOverlayCapacity {
			rows: 6,
			columns: 20
		}
	);
}

#[test]
fn scaled_geometry_stays_inside_the_panel_and_uses_opaque_palette_colors() {
	let style = ScreenOverlayStyle {
		text_scale: 1.5,
		high_contrast: true,
	};
	let geometry = OverlayLayout::new([460, 225], 1.5, style).unwrap();
	let [columns, rows] = [geometry.capacity.columns, geometry.capacity.rows];
	let text = vec!["X".repeat(columns); rows];
	let mesh = geometry.draw(&text, style, ScreenOverlayLayout::Standard);
	let (background, characters) = mesh.vertices().split_at(4);
	assert!(!characters.is_empty());
	assert!(background.iter().all(|corner| corner.normal == [0.0; 3]));
	assert!(characters.iter().all(|corner| corner.normal == [1.0; 3]));
	for vertex in characters {
		assert!(vertex.position[0] >= background[0].position[0]);
		assert!(vertex.position[0] <= background[2].position[0]);
		assert!(vertex.position[1] >= background[0].position[1]);
		assert!(vertex.position[1] <= background[2].position[1]);
	}
}

#[test]
fn matching_visible_rows_do_not_hide_a_palette_or_scale_change() {
	let key = OverlayKey {
		extent: [640, 360],
		dpi_bits: 1.0f64.to_bits(),
		style: ScreenOverlayStyle::default(),
		layout: ScreenOverlayLayout::Standard,
	};
	let prepared = PreparedOverlay {
		key,
		rows: vec!["Hello".into()],
	};
	assert!(prepared.matches(key, &["Hello world".into()], 5));
	assert!(!prepared.matches(key, &["Hints".into()], 5));
	let compact = OverlayKey {
		layout: ScreenOverlayLayout::Compact {
			bottom: true,
			aim: false,
		},
		..key
	};
	assert!(!prepared.matches(compact, &["Hello".into()], 5));
	for style in [
		ScreenOverlayStyle {
			high_contrast: true,
			..key.style
		},
		ScreenOverlayStyle {
			text_scale: 1.25,
			..key.style
		},
	] {
		assert!(!prepared.matches(OverlayKey { style, ..key }, &["Hello".into()], 5));
	}
}
