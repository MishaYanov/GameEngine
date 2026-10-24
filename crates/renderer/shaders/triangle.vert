#version 450

layout(location = 0) in vec3 inPosition;
layout(location = 1) in vec2 inUv;
layout(location = 2) in vec3 inNormal;
layout(location = 3) in vec4 instanceColumn0;
layout(location = 4) in vec4 instanceColumn1;
layout(location = 5) in vec4 instanceColumn2;
layout(location = 6) in vec4 instanceColumn3;
layout(location = 7) in vec4 instanceTint;

layout(location = 0) out vec2 outUv;
layout(location = 1) out vec3 outNormal;
layout(location = 2) out vec3 outWorldPosition;
layout(location = 3) out vec4 outTint;

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

	bool instanced = (modelData.flags.x & 16u) != 0u;
	mat4 model = instanced ? mat4(instanceColumn0, instanceColumn1, instanceColumn2, instanceColumn3) : modelData.model;
	outTint = instanced ? instanceTint : modelData.baseColorFactor;

	if ((modelData.flags.x & 2u) != 0u) {
		gl_Position = model * vec4(inPosition, 1.0);
		outNormal = inNormal; // Overlay vertex color.
		outUv = inUv;
		outWorldPosition = vec3(0);
		return;
	}
	gl_Position =
		camera.viewProjection
		* model
		* vec4(
			inPosition,
			1.0
		);

	outUv = inUv;
	outWorldPosition = (model * vec4(inPosition, 1.0)).xyz;
	mat3 linear = mat3(model);
	// A collapsed transform has no inverse; retain a finite fallback normal.
	outNormal = (modelData.flags.x & 4u) != 0u ? inNormal
		: (abs(determinant(linear)) > 1e-12 ? transpose(inverse(linear)) * inNormal : inNormal);
}
