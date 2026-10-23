#version 450

layout(location = 0) in vec3 inPosition;
layout(location = 1) in vec2 inUv;
layout(location = 2) in vec3 inNormal;

layout(location = 0) out vec2 outUv;
layout(location = 1) out vec3 outNormal;

layout(set = 0, binding = 0)
uniform CameraUniform {
	mat4 viewProjection;
} camera;

layout(push_constant, std430)
uniform ModelPushConstants {
	mat4 model;
	vec4 baseColorFactor;
    uvec4 flags;
} modelData;

void main() {
	if ((modelData.flags.x & 2u) != 0u) {
        gl_Position = modelData.model * vec4(inPosition, 1.0);
        outNormal = inNormal; // Overlay vertex color.
        outUv = inUv;
        return;
    }
	gl_Position =
		camera.viewProjection
		* modelData.model
		* vec4(
			inPosition,
			1.0
		);

	outUv = inUv;
    mat3 linear = mat3(modelData.model);
    // A collapsed transform has no inverse; retain a finite fallback normal.
    outNormal = (modelData.flags.x & 4u) != 0u ? inNormal
        : (abs(determinant(linear)) > 1e-12 ? transpose(inverse(linear)) * inNormal : inNormal);
}
