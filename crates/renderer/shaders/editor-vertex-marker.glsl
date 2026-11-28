vec4 editorVertexMarker(vec4 vertex, uint viewportExtent, out vec2 corner) {
	const vec2 corners[6] = vec2[6](
		vec2(-1, -1), vec2(1, -1), vec2(-1, 1),
		vec2(-1, 1), vec2(1, -1), vec2(1, 1)
	);
	corner = corners[gl_VertexIndex];
	vec2 viewport = vec2(viewportExtent & 65535u, viewportExtent >> 16u);
	// Expand by three pixels with perspective-independent screen size.
	vertex.xy += corner * (6.0 / max(viewport, vec2(1.0))) * vertex.w;
	if (vertex.w > 0.0 && vertex.z > 0.0 && vertex.z <= vertex.w) {
		// Keep a marker on its own surface visible through depth rounding.
		vertex.z = max(0.0, vertex.z - vertex.w * 0.000001);
	}
	return vertex;
}
