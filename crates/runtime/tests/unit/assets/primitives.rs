use super::super::MeshData;

#[test]
fn primitives_have_valid_uvs_and_outward_nondegenerate_triangles() {
	for (mesh, rounded) in [
		(MeshData::cube(), false),
		(MeshData::plane(), false),
		(MeshData::polygon(5), false),
		(MeshData::sphere(), true),
		(MeshData::capsule(), true),
	] {
		assert_eq!(mesh.indices().len() % 3, 0);
		for vertex in mesh.vertices() {
			assert!(vertex.position.iter().all(|value| value.is_finite()));
			assert!(vertex.uv.iter().all(|value| (0.0..=1.0).contains(value)));
		}
		for triangle in mesh.indices().chunks_exact(3) {
			let [a, b, c] =
				std::array::from_fn::<_, 3, _>(|i| mesh.vertices()[triangle[i] as usize].position);
			let u = std::array::from_fn::<_, 3, _>(|i| b[i] - a[i]);
			let v = std::array::from_fn::<_, 3, _>(|i| c[i] - a[i]);
			let normal = [
				u[1] * v[2] - u[2] * v[1],
				u[2] * v[0] - u[0] * v[2],
				u[0] * v[1] - u[1] * v[0],
			];
			assert!(normal.iter().map(|x| x * x).sum::<f32>() > 1e-12);
			if rounded {
				let center = std::array::from_fn::<_, 3, _>(|i| (a[i] + b[i] + c[i]) / 3.0);
				assert!((0..3).map(|i| normal[i] * center[i]).sum::<f32>() > 0.0);
			}
		}
	}
}

#[test]
#[should_panic(expected = "a polygon needs at least three sides")]
fn polygon_rejects_invalid_side_count() {
	MeshData::polygon(2);
}
