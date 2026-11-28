#version 450
#extension GL_GOOGLE_include_directive : require
#ifdef EDITOR_VERTEX_MARKERS
#include "editor-vertex-marker.glsl"
#endif
layout(location=0) in vec3 inPosition;
#ifndef EDITOR_VERTEX_MARKERS
layout(location=1) in vec2 inUv;
#ifndef DIRECTIONAL_SHADOW
layout(location=2) in vec3 inNormal;
layout(location=8) in vec4 inTangent;
layout(location=9) in vec2 inNormalUv;
#include "normal-map.glsl"
#endif
#endif
layout(location=3) in uvec4 inJoints;
layout(location=4) in vec4 inWeights;
layout(location=0) out vec2 outUv;
layout(location=1) out vec3 outNormal;
layout(location=2) out vec3 outWorldPosition;
layout(location=3) out vec4 outTint;
#ifndef DIRECTIONAL_SHADOW
layout(location=4) out vec4 outTangent;
layout(location=5) out vec2 outNormalUv;
#endif
layout(set=0,binding=0) uniform CameraUniform { mat4 viewProjection; } camera;
layout(set=2,binding=0,std430) readonly buffer SkinPalette { mat4 joints[]; } skin;
// Prefix of the push constants shared by the material passes.
// flags.x remains material/render flags; y/z are palette base/count.
#ifdef DIRECTIONAL_SHADOW
layout(set = 0, binding = 2, std140) uniform ShadowUniform {
	mat4 lightViewProjection;
	vec4 lightInfo;
} shadow;
#endif

layout(push_constant,std430) uniform ModelPushConstants {
    mat4 model; vec4 baseColorFactor; uvec4 flags;
} modelData;
void main() {
    vec4 p = vec4(0);
#ifndef DIRECTIONAL_SHADOW
    outTangent = vec4(0.0);
    outNormalUv = vec2(0.0);
#endif
#if !defined(EDITOR_VERTEX_MARKERS) && !defined(DIRECTIONAL_SHADOW)
    vec3 normal = vec3(0.0);
    vec3 dominantNormal = inNormal;
    vec3 tangent = vec3(0.0);
    vec3 dominantTangent = inTangent.xyz;
    float highest = -1.0;
    bool mapped = (modelData.flags.x & NORMAL_MAP_FLAG) != 0u && abs(inTangent.w) > 0.5;
#endif
    for (int i=0;i<4;i++) {
        uint index = modelData.flags.y + min(inJoints[i], modelData.flags.z-1u);
        mat4 joint = skin.joints[index];
        p += inWeights[i] * (joint * vec4(inPosition,1));
#if !defined(EDITOR_VERTEX_MARKERS) && !defined(DIRECTIONAL_SHADOW)
        // Each palette entry is a checked positive similarity. Removing its
        // uniform scale avoids inverting the potentially singular LBS blend.
        vec3 n = normalize(mat3(joint) * inNormal);
        vec3 t = mapped ? normalize(mat3(joint) * inTangent.xyz) : vec3(0.0);
        normal += inWeights[i] * n;
        tangent += inWeights[i] * t;
        if (inWeights[i] > highest) {
            highest = inWeights[i];
            dominantNormal = n;
            dominantTangent = t;
        }
#endif
    }
    vec4 world = modelData.model * p;
#ifdef EDITOR_VERTEX_MARKERS
    gl_Position = editorVertexMarker(camera.viewProjection * world, modelData.flags.w, outUv);
    outNormal = vec3(0.0);
#elif defined(DIRECTIONAL_SHADOW)
    gl_Position = shadow.lightViewProjection * world;
    outUv = inUv;
    outNormal = vec3(0.0);
#else
    // Opposite valid rotations may cancel. Stable largest-weight fallback;
    // ties select the first influence, independent of GPU scheduling.
    normal = dot(normal, normal) > 1e-12 ? normalize(normal) : dominantNormal;
    gl_Position = camera.viewProjection * world;
    mat3 linear = mat3(modelData.model);
    outNormal = normalize(linear * normal);
    outUv = inUv;
    if (mapped) {
        tangent = dot(tangent, tangent) > 1e-12 ? normalize(tangent) : dominantTangent;
        vec3 projected = tangent - normal * dot(normal, tangent);
        if (dot(projected, projected) <= 1e-12) tangent = dominantTangent;
        outTangent = modelTangent(linear, determinant(linear), vec4(tangent, inTangent.w));
        outNormalUv = inNormalUv;
    }
#endif
    outWorldPosition = world.xyz;
    outTint = modelData.baseColorFactor;
}
