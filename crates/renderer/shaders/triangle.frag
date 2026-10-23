#version 450

layout(location = 0) in vec2 inUv;
layout(location = 1) in vec3 inNormal;

layout(location = 0) out vec4 outColor;

layout(set = 1, binding = 0)
uniform sampler2D textureSampler;

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
		) * modelData.baseColorFactor;
    if ((modelData.flags.x & 2u) != 0u) {
        // Bitmap atlas is opaque white for foreground and black for empty cells.
        if (outColor.r < 0.5) discard;
        outColor = vec4(inNormal * modelData.baseColorFactor.rgb, 1.0);
    } else if ((modelData.flags.x & 4u) != 0u) {
        outColor.rgb *= inNormal;
    } else if ((modelData.flags.x & 1u) == 0u) {
        vec3 normal = length(inNormal) > 1e-8 ? normalize(inNormal) : vec3(0,0,1);
        if (!gl_FrontFacing) normal = -normal;
        vec3 toLight = normalize(vec3(-0.4, 0.8, 0.6));
        float diffuse = max(dot(normal, toLight), 0.0);
        outColor.rgb *= 0.25 + 0.75 * diffuse;
    }
}
