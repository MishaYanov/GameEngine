#version 450

layout(location = 0) in vec2 inUv;
layout(location = 1) in vec3 inNormal;
layout(location = 2) in vec3 inWorldPosition;
layout(location = 3) in vec4 inTint;

layout(location = 0) out vec4 outColor;

layout(set = 1, binding = 0)
uniform sampler2D textureSampler;

struct Light {
	vec4 positionKind;
	vec4 directionRange;
	vec4 colorIntensity;
};
layout(set = 0, binding = 1, std140)
uniform LightingUniform {
	vec4 ambientCount;
	Light lights[16];
} lighting;

layout(push_constant, std430)
uniform ModelPushConstants {
	mat4 model;
	vec4 baseColorFactor;
	uvec4 flags;
} modelData;

void main() {
	outColor =
		texture(
			textureSampler,
			inUv
		) * inTint;
	if ((modelData.flags.x & 2u) != 0u) {
		// Bitmap atlas is opaque white for foreground and black for empty cells.
		if (outColor.r < 0.5) discard;
		outColor = vec4(inNormal * inTint.rgb, 1.0);
	} else if ((modelData.flags.x & 4u) != 0u) {
		outColor.rgb *= inNormal;
	} else if ((modelData.flags.x & 1u) == 0u) {
		vec3 normal = length(inNormal) > 1e-8 ? normalize(inNormal) : vec3(0,0,1);
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
			irradiance += light.colorIntensity.rgb * light.colorIntensity.w
				* attenuation * max(dot(normal, toLight), 0.0);
		}
		outColor.rgb *= irradiance;
	}
}
