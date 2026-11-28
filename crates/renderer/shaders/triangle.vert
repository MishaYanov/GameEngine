#version 450

#extension GL_GOOGLE_include_directive : require
#ifdef EDITOR_VERTEX_MARKERS
#include "editor-vertex-marker.glsl"
#endif

layout(location = 0) in vec3 inPosition;
#ifndef EDITOR_VERTEX_MARKERS
layout(location = 1) in vec2 inUv;
#ifndef DIRECTIONAL_SHADOW
layout(location = 2) in vec3 inNormal;
layout(location = 8) in vec4 inTangent;
layout(location = 9) in vec2 inNormalUv;
#include "normal-map.glsl"
#endif
layout(location = 3) in vec4 instanceColumn0;
layout(location = 4) in vec4 instanceColumn1;
layout(location = 5) in vec4 instanceColumn2;
layout(location = 6) in vec4 instanceColumn3;
layout(location = 7) in vec4 instanceTint;
#endif

layout(location = 0) out vec2 outUv;
layout(location = 1) out vec3 outNormal;
layout(location = 2) out vec3 outWorldPosition;
layout(location = 3) out vec4 outTint;
#ifndef DIRECTIONAL_SHADOW
layout(location = 4) out vec4 outTangent;
layout(location = 5) out vec2 outNormalUv;
#endif

layout(set = 0, binding = 0)
uniform CameraUniform {
	mat4 viewProjection;
} camera;

#ifdef DIRECTIONAL_SHADOW
layout(set = 0, binding = 2, std140) uniform ShadowUniform {
	mat4 lightViewProjection;
	vec4 lightInfo;
} shadow;
#endif

layout(push_constant, std430)
uniform ModelPushConstants {
	mat4 model;
	vec4 baseColorFactor;
	uvec4 flags;
} modelData;

void main() {
#ifndef DIRECTIONAL_SHADOW
	outTangent = vec4(0.0);
	outNormalUv = vec2(0.0);
#endif
#ifdef EDITOR_VERTEX_MARKERS
	vec4 vertex = camera.viewProjection * modelData.model * vec4(inPosition, 1.0);
	gl_Position = editorVertexMarker(vertex, modelData.flags.w, outUv);
	outNormal = vec3(0.0);
	outWorldPosition = vec3(0.0);
	outTint = modelData.baseColorFactor;
#else
	bool instanced = (modelData.flags.x & 16u) != 0u;
	mat4 model = instanced ? mat4(instanceColumn0, instanceColumn1, instanceColumn2, instanceColumn3) : modelData.model;
	outTint = instanced ? instanceTint : modelData.baseColorFactor;

	#ifndef DIRECTIONAL_SHADOW
	if ((modelData.flags.x & 2u) != 0u) {
		gl_Position = model * vec4(inPosition, 1.0);
		outNormal = inNormal; // Overlay vertex color.
		outUv = inUv;
		outWorldPosition = vec3(0);
		return;
	}
	#endif
	vec4 world = model * vec4(inPosition, 1.0);
	outUv = inUv;
#ifdef DIRECTIONAL_SHADOW
	gl_Position = shadow.lightViewProjection * world;
	outNormal = vec3(0.0);
	outWorldPosition = vec3(0.0);
#else
	gl_Position = camera.viewProjection * world;
	outWorldPosition = world.xyz;
	mat3 linear = mat3(model);
	float determinantValue = determinant(linear);
	// A collapsed transform has no inverse; retain a finite fallback normal.
	outNormal = (modelData.flags.x & 4u) != 0u ? inNormal
		: (abs(determinantValue) > 1e-12 ? transpose(inverse(linear)) * inNormal : inNormal);
	if ((modelData.flags.x & NORMAL_MAP_FLAG) != 0u) {
		outTangent = modelTangent(linear, determinantValue, inTangent);
		outNormalUv = inNormalUv;
	}
#endif
#endif
}
