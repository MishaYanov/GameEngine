#version 450
#extension GL_GOOGLE_include_directive : require
#include "normal-map.glsl"

layout(location = 0) in vec2 inUv;
layout(location = 1) in vec3 inNormal;
layout(location = 2) in vec3 inWorldPosition;
layout(location = 3) in vec4 inTint;
layout(location = 4) in vec4 inTangent;
layout(location = 5) in vec2 inNormalUv;

layout(location = 0) out vec4 outColor;

layout(set = 1, binding = 0)
uniform sampler2D textureSampler;
layout(set = 3, binding = 0) uniform sampler2D normalSampler;

struct Light {
	vec4 positionKind;
	vec4 directionRange;
	vec4 colorIntensity;
};
layout(set = 0, binding = 1, std140)
uniform LightingUniform {
	vec4 ambientCount;
	Light lights[16];
	vec4 fogColorDensity;
	vec4 fogSettings;
	vec4 fogCameraOrigin;
	vec4 fogCameraForward;
} lighting;

layout(set = 0, binding = 2, std140) uniform ShadowUniform {
	mat4 lightViewProjection;
	vec4 lightInfo;
} shadow;

layout(set = 0, binding = 3) uniform sampler2D shadowDepth;

layout(push_constant, std430)
uniform ModelPushConstants {
	mat4 model;
	vec4 baseColorFactor;
	uvec4 flags;
	vec4 emissiveFactor;
} modelData;

// Keep the direct light visible outside the map and fade its last few texels.
float directionalVisibility(float cosine, vec3 receiverNormal) {
	vec3 clip = (shadow.lightViewProjection * vec4(inWorldPosition, 1.0)).xyz;
	vec2 uv = clip.xy * 0.5 + 0.5;
	vec2 edge = min(uv, 1.0 - uv);
	if (min(edge.x, edge.y) <= 0.0 || clip.z <= 0.0 || clip.z >= 1.0) return 1.0;
	// Fit the comparison plane to the receiver at each actual texel center.
	// A fixed depth for all PCF taps makes long, shallow slopes shadow themselves.
	mat3 basis = transpose(mat3(shadow.lightViewProjection));
	vec3 lightPlane = vec3(
		dot(receiverNormal, basis[0]) / dot(basis[0], basis[0]),
		dot(receiverNormal, basis[1]) / dot(basis[1], basis[1]),
		dot(receiverNormal, basis[2]) / dot(basis[2], basis[2]));
	vec2 gradient = abs(lightPlane.z) > 1e-6 ? -2.0 * lightPlane.xy / lightPlane.z : vec2(0.0);
	float bias = mix(0.00040, 0.00014, clamp(cosine, 0.0, 1.0));
	ivec2 center = ivec2(uv / shadow.lightInfo.yz);
	float visibility = 0.0;
	for (int y = -1; y <= 1; ++y) {
		for (int x = -1; x <= 1; ++x) {
			ivec2 pixel = center + ivec2(x, y);
			if (any(lessThan(pixel, ivec2(0))) || any(greaterThanEqual(pixel, textureSize(shadowDepth, 0)))) {
				visibility += 1.0;
				continue;
			}
			vec2 sampleUv = (vec2(pixel) + 0.5) * shadow.lightInfo.yz;
			float correction = clamp(dot(gradient, sampleUv - uv), -0.004, 0.004);
			float nearest = texelFetch(shadowDepth, pixel, 0).r;
			visibility += step(clip.z + correction - bias, nearest);
		}
	}
	float fade = smoothstep(0.0, 8.0 * shadow.lightInfo.y, min(edge.x, edge.y));
	return mix(1.0, visibility / 9.0, fade);
}

// Integrate an exponential height profile along the visible part of the ray.
vec3 applyFog(vec3 surfaceColor) {
	float density = lighting.fogColorDensity.w;
	if (density <= 0.0) return surfaceColor;
	vec3 ray = inWorldPosition - lighting.fogCameraOrigin.xyz;
	if (dot(lighting.fogCameraForward.xyz, lighting.fogCameraForward.xyz) > 0.5) {
		vec3 forward = lighting.fogCameraForward.xyz;
		ray = forward * max(dot(forward, ray), 0.0);
	}
	float distance = length(ray);
	float start = lighting.fogSettings.z;
	if (distance <= start) return surfaceColor;

	float base = lighting.fogSettings.x;
	float falloff = lighting.fogSettings.y;
	float h0 = inWorldPosition.y - ray.y * (1.0 - start / distance) - base;
	float h1 = inWorldPosition.y - base;
	float low = min(h0, h1);
	float high = max(h0, h1);
	float meanDensity = 1.0;
	if (high > 0.0 && falloff > 0.0) {
		float altitude = max(low, 0.0);
		float span = (high - altitude) * falloff;
		float integral = span > 1e-3 ? (1.0 - exp(-span)) / span : 1.0 - 0.5 * span;
		float aboveBase = low < 0.0 ? high / (high - low) : 1.0;
		meanDensity = mix(1.0, exp(-altitude * falloff) * integral, aboveBase);
	}
	float opacity = 1.0 - exp(-density * (distance - start) * meanDensity);
	return mix(surfaceColor, lighting.fogColorDensity.rgb, opacity);
}

void main() {
	if ((modelData.flags.x & 64u) != 0u) {
		float distance = abs(inUv.x) + abs(inUv.y);
		if (distance > 1.0) discard;
		outColor = distance > 0.70 ? vec4(0.025, 0.025, 0.025, 1.0) : vec4(1.0, 0.70, 0.23, 1.0);
		return;
	}
	if ((modelData.flags.x & 128u) != 0u) {
		// A quiet, directional editor light makes the occluding surface readable.
		vec3 normal = length(inNormal) > 1e-8 ? normalize(inNormal) : vec3(0.0, 1.0, 0.0);
		if (!gl_FrontFacing) normal = -normal;
		float light = max(dot(normal, normalize(vec3(-0.45, 0.82, 0.35))), 0.0);
		outColor = vec4(mix(vec3(0.035, 0.045, 0.064), vec3(0.113, 0.145, 0.193), light), 1.0);
		return;
	}
	if ((modelData.flags.x & 32u) != 0u) {
		// Reciprocal clip W gives camera distance for a perspective viewport.
		float distance = 1.0 / max(gl_FragCoord.w, 1e-6);
		float proximity = 1.0 - smoothstep(4.0, 60.0, distance);
		vec3 edge = mix(vec3(0.24, 0.34, 0.48), vec3(0.48, 0.67, 0.89), proximity);
		outColor = vec4(edge, 1.0);
		return;
	}
	// Evaluate derivatives before the alpha mask diverges across the fragment quad.
	vec3 receiverNormal = cross(dFdx(inWorldPosition), dFdy(inWorldPosition));
	vec2 normalDx = dFdx(inNormalUv);
	vec2 normalDy = dFdy(inNormalUv);
	outColor =
		texture(
			textureSampler,
			inUv
		) * inTint;
	if ((modelData.flags.x & 256u) != 0u) {
		bool maskedMaterial = (modelData.flags.x & 512u) != 0u;
		float cutoff = maskedMaterial ? uintBitsToFloat(modelData.flags.w) : 0.5;
		if (outColor.a < cutoff) discard;
		if (maskedMaterial) outColor.a = 1.0;
	}
	if ((modelData.flags.x & 2u) != 0u) {
		// Bitmap atlas is opaque white for foreground and black for empty cells.
		if (outColor.r < 0.5) discard;
		outColor = vec4(inNormal * inTint.rgb, 1.0);
	} else if ((modelData.flags.x & 4u) != 0u) {
		outColor.rgb *= inNormal;
	} else if ((modelData.flags.x & 1u) == 0u) {
		vec3 normal = length(inNormal) > 1e-8 ? normalize(inNormal) : vec3(0,0,1);
		if ((modelData.flags.x & NORMAL_MAP_FLAG) != 0u && abs(inTangent.w) > 0.5) {
			vec3 sampleNormal = textureGrad(normalSampler, inNormalUv, normalDx, normalDy).rgb;
			normal = mappedNormal(normal, inTangent, sampleNormal, modelData.emissiveFactor.w);
		}
		if (!gl_FrontFacing) normal = -normal;
		vec3 irradiance = lighting.ambientCount.rgb;
		int count = clamp(int(lighting.ambientCount.w), 0, 16);
		for (int i = 0; i < count; ++i) {
			Light light = lighting.lights[i];
			vec3 toLight;
			float attenuation = 1.0;
			if (light.positionKind.w < 0.5) {
				toLight = -light.directionRange.xyz;
			} else {
				vec3 delta = light.positionKind.xyz - inWorldPosition;
				float distance = length(delta);
				toLight = distance > 1e-8 ? delta / distance : normal;
				float falloff = max(1.0 - distance / light.directionRange.w, 0.0);
				attenuation = falloff * falloff;
			}
			float cosine = max(dot(normal, toLight), 0.0);
			if (cosine > 0.0 && i + 1 == int(shadow.lightInfo.x)) {
				float receiverLen2 = dot(receiverNormal, receiverNormal);
				float slopeCosine = receiverLen2 > 1e-12
					? abs(dot(receiverNormal, toLight)) * inversesqrt(receiverLen2) : 1.0;
				attenuation *= directionalVisibility(slopeCosine, receiverNormal);
			}
			irradiance += light.colorIntensity.rgb * light.colorIntensity.w
				* attenuation * cosine;
		}
		outColor.rgb *= irradiance;
	}
	if ((modelData.flags.x & (2u | 4u | 8u)) == 0u) {
		outColor.rgb = applyFog(outColor.rgb + modelData.emissiveFactor.rgb);
	}
}
