use runtime::GameContentRect;

#[test]
fn game_content_rect_keeps_portrait_landscape_and_the_exact_boundary() {
	for [width, height] in [[1280, 720], [720, 1280], [3000, 1000], [1, 1]] {
		let rect = GameContentRect::fit(width, height).unwrap();
		assert_eq!(rect.pixels(), [0, 0, width, height]);
		assert_eq!(rect.aspect(), width as f32 / height as f32);
	}
	for [width, height] in [[0, 1], [1, 0], [u32::MAX, 1], [1, u32::MAX]] {
		assert!(GameContentRect::fit(width, height).is_none());
	}
	assert_eq!(
		GameContentRect::fit(i32::MAX as u32, 1).unwrap().aspect(),
		3.0
	);
}

#[test]
fn game_content_rect_centers_odd_bars_and_maps_half_open_content_pixels() {
	let rect = GameContentRect::fit(4001, 1000).unwrap();
	assert_eq!(rect.pixels(), [500, 0, 3000, 1000]);
	assert_eq!(rect.extent(), [3000, 1000]);
	assert_eq!(rect.aspect(), 3.0);
	assert_eq!(rect.local_pointer([500.0, 0.0]), Some([0.0, 0.0]));
	assert_eq!(rect.local_pointer([3499.5, 999.5]), Some([2999.5, 999.5]));
	for point in [
		[499.5, 10.0],
		[3500.0, 10.0],
		[500.0, 1000.0],
		[f64::NAN, 0.0],
		[500.0, f64::INFINITY],
	] {
		assert_eq!(rect.local_pointer(point), None);
	}
}
