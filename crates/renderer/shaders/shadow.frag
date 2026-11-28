#version 450

layout(location = 0) in vec2 inUv;
layout(location = 3) in vec4 inTint;

layout(set = 1, binding = 0) uniform sampler2D textureSampler;
layout(push_constant, std430) uniform ModelPushConstants {
    mat4 model;
    vec4 baseColorFactor;
    uvec4 flags;
} modelData;

void main() {
    if ((modelData.flags.x & 256u) != 0u) {
        float cutoff = (modelData.flags.x & 512u) != 0u ? uintBitsToFloat(modelData.flags.w) : 0.5;
        if (texture(textureSampler, inUv).a * inTint.a < cutoff) discard;
    }
}
