#version 450

layout(location = 0) in vec3 inPosition;
layout(location = 1) in vec2 inUv;

layout(location = 0) out vec2 outUv;

layout(set = 0, binding = 0)
uniform CameraUniform {
	mat4 viewProjection;
} camera;

layout(push_constant)
uniform ModelPushConstants {
	mat4 model;
} modelData;

void main() {
	gl_Position =
		camera.viewProjection
		* modelData.model
		* vec4(
			inPosition,
			1.0
		);

	outUv = inUv;
}